//! sirtone-net — the one network layer (T3.1, FR-REC-007; `03` §11, `08` §4): an allowlist of
//! hosts with their rate limits, cache lifetimes and size caps; HTTPS only; a descriptive
//! User-Agent (MusicBrainz/ListenBrainz rule); the response cache (`HttpCache` port, the
//! library's `http_cache` table); the offline manager — the «Работать полностью офлайн» master
//! switch blocks **every** request, and three network failures in a row mean offline until a
//! probe (every 60 s) gets through. No telemetry: these hosts are the whole outgoing list.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::time::Duration;

use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::ports::HttpCache;

/// Every host the app may call (`08` §4). The updater talks to GitHub through Tauri's plugin
/// and asks [`Net::is_offline`] first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Host {
    AcoustId,
    MusicBrainz,
    CoverArt,
    OpenMeteo,
    Geocoding,
    ListenBrainz,
}

/// How a host is called (`03` §11).
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    pub base: &'static str,
    /// The least time between two requests.
    pub interval: Duration,
    pub cache_ttl_s: i64,
    pub max_bytes: usize,
}

const DAY: i64 = 24 * 3600;
const MB: usize = 1024 * 1024;

impl Host {
    pub fn policy(self) -> Policy {
        let p = |base, ms, ttl, max| Policy {
            base,
            interval: Duration::from_millis(ms),
            cache_ttl_s: ttl,
            max_bytes: max,
        };
        match self {
            // ≤ 2 req/s (the service allows 3).
            Self::AcoustId => p("https://api.acoustid.org/v2/", 500, 90 * DAY, MB),
            // ≤ 0.8 req/s, 503 beyond.
            Self::MusicBrainz => p("https://musicbrainz.org/ws/2/", 1250, 30 * DAY, MB),
            Self::CoverArt => p("https://coverartarchive.org/", 250, 90 * DAY, 10 * MB),
            // Weather is «stale» after 3 h.
            Self::OpenMeteo => p("https://api.open-meteo.com/v1/", 1000, 3 * 3600, MB),
            Self::Geocoding => p(
                "https://geocoding-api.open-meteo.com/v1/",
                1000,
                365 * DAY,
                MB,
            ),
            Self::ListenBrainz => p("https://api.listenbrainz.org/1/", 1000, 30 * DAY, MB),
        }
    }

    const ALL: [Host; 6] = [
        Self::AcoustId,
        Self::MusicBrainz,
        Self::CoverArt,
        Self::OpenMeteo,
        Self::Geocoding,
        Self::ListenBrainz,
    ];
}

/// One response.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

/// How bytes travel (reqwest in the app, a fake in tests).
pub trait Transport: Send + Sync {
    /// GET `url` with the User-Agent; at most `max_bytes` of body (more is an error).
    fn get(&self, url: &str, user_agent: &str, max_bytes: usize) -> Result<Response, NetError>;
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NetError {
    #[error("offline")]
    Offline,
    #[error("network: {0}")]
    Network(String),
    #[error("http {0}")]
    Status(u16),
    #[error("response too large")]
    TooLarge,
    #[error("not allowed: {0}")]
    NotAllowed(String),
}

impl From<NetError> for AppError {
    fn from(e: NetError) -> Self {
        let code = match &e {
            NetError::Offline | NetError::Network(_) => ErrorCode::NetworkOffline,
            NetError::Status(429 | 503) => ErrorCode::RateLimited,
            NetError::Status(_) | NetError::TooLarge => ErrorCode::RemoteError,
            NetError::NotAllowed(_) => ErrorCode::InvalidArgument,
        };
        let retry = matches!(code, ErrorCode::NetworkOffline | ErrorCode::RateLimited);
        let mut err = AppError::new(code).with_details(e.to_string());
        err.retryable = retry;
        err
    }
}

/// Network failures in a row before «offline», and the probe interval after.
const FAILS_OFFLINE: u32 = 3;
const PROBE_MS: i64 = 60_000;

/// The network state shared by everything that calls out.
pub struct Net {
    transport: Box<dyn Transport>,
    cache: Option<Box<dyn HttpCache>>,
    user_agent: String,
    /// «Работать полностью офлайн» (`network.offline_mode`).
    forced: AtomicBool,
    fails: AtomicU32,
    /// Offline (by failures) until this time; 0 — online.
    down_until: AtomicI64,
    next_at: Mutex<Vec<(Host, i64)>>,
    now: Box<dyn Fn() -> i64 + Send + Sync>,
    sleep: Box<dyn Fn(Duration) + Send + Sync>,
}

impl Net {
    /// `user_agent` — `SirTone/<version> ( <contact> )` (the MusicBrainz rule).
    pub fn new(transport: Box<dyn Transport>, user_agent: String) -> Self {
        Self {
            transport,
            cache: None,
            user_agent,
            forced: AtomicBool::new(false),
            fails: AtomicU32::new(0),
            down_until: AtomicI64::new(0),
            next_at: Mutex::new(Host::ALL.iter().map(|&h| (h, 0)).collect()),
            now: Box::new(now_ms),
            sleep: Box::new(std::thread::sleep),
        }
    }

    pub fn with_cache(mut self, cache: Box<dyn HttpCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Time and sleep for tests (a fake clock).
    pub fn with_clock(
        mut self,
        now: impl Fn() -> i64 + Send + Sync + 'static,
        sleep: impl Fn(Duration) + Send + Sync + 'static,
    ) -> Self {
        self.now = Box::new(now);
        self.sleep = Box::new(sleep);
        self
    }

    /// The master switch.
    pub fn set_offline(&self, on: bool) {
        self.forced.store(on, Ordering::Relaxed);
    }

    pub fn forced_offline(&self) -> bool {
        self.forced.load(Ordering::Relaxed)
    }

    /// Offline now: by the switch, or after failures until the next probe.
    pub fn is_offline(&self) -> bool {
        self.forced_offline() || (self.now)() < self.down_until.load(Ordering::Relaxed)
    }

    /// GET `path` (relative to the host's base) — from the cache while fresh, else over the
    /// network within the host's rate; errors never touch the cache.
    pub fn get(&self, host: Host, path: &str) -> Result<Vec<u8>, NetError> {
        let p = host.policy();
        if path.contains("://") || path.starts_with('/') || path.contains("..") {
            return Err(NetError::NotAllowed(path.into()));
        }
        let url = format!("{}{path}", p.base);
        let now = (self.now)();
        if let Some(c) = &self.cache
            && let Some(body) = c.get(&url, now)
        {
            return Ok(body);
        }
        if self.is_offline() {
            return Err(NetError::Offline);
        }
        self.wait_turn(host, p.interval);
        match self.transport.get(&url, &self.user_agent, p.max_bytes) {
            Ok(r) if (200..300).contains(&r.status) => {
                self.fails.store(0, Ordering::Relaxed);
                self.down_until.store(0, Ordering::Relaxed);
                if let Some(c) = &self.cache {
                    c.put(&url, &r.body, (self.now)(), p.cache_ttl_s);
                }
                Ok(r.body)
            }
            Ok(r) => {
                // The server answered: the network is up.
                self.fails.store(0, Ordering::Relaxed);
                Err(NetError::Status(r.status))
            }
            Err(NetError::Network(e)) => {
                if self.fails.fetch_add(1, Ordering::Relaxed) + 1 >= FAILS_OFFLINE {
                    self.down_until
                        .store((self.now)() + PROBE_MS, Ordering::Relaxed);
                }
                Err(NetError::Network(e))
            }
            Err(e) => Err(e),
        }
    }

    /// The host's rate: waits until its next slot, then books the one after.
    fn wait_turn(&self, host: Host, interval: Duration) {
        let wait = {
            let Ok(mut slots) = self.next_at.lock() else {
                return;
            };
            let now = (self.now)();
            let Some(slot) = slots.iter_mut().find(|(h, _)| *h == host) else {
                return;
            };
            let at = slot.1.max(now);
            slot.1 = at + interval.as_millis() as i64;
            at - now
        };
        if wait > 0 {
            (self.sleep)(Duration::from_millis(wait as u64));
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// The real transport: reqwest over SChannel (no OpenSSL), system proxy and certificates;
/// connect 5 s, total 20 s; redirects only to HTTPS (≤ 5).
pub struct Reqwest(reqwest::blocking::Client);

impl Reqwest {
    pub fn new() -> Result<Self, NetError> {
        let policy = reqwest::redirect::Policy::custom(|a| {
            if a.url().scheme() == "https" && a.previous().len() < 5 {
                a.follow()
            } else {
                a.stop()
            }
        });
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .redirect(policy)
            .https_only(true)
            .build()
            .map(Self)
            .map_err(|e| NetError::Network(e.to_string()))
    }
}

impl Transport for Reqwest {
    fn get(&self, url: &str, user_agent: &str, max_bytes: usize) -> Result<Response, NetError> {
        use std::io::Read;
        let resp = self
            .0
            .get(url)
            .header(reqwest::header::USER_AGENT, user_agent)
            .send()
            .map_err(|e| NetError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let mut body = Vec::new();
        resp.take(max_bytes as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|e| NetError::Network(e.to_string()))?;
        if body.len() > max_bytes {
            return Err(NetError::TooLarge);
        }
        Ok(Response { status, body })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    /// Counts calls; answers from a script.
    struct Fake {
        calls: Arc<AtomicUsize>,
        answer: Box<dyn Fn() -> Result<Response, NetError> + Send + Sync>,
    }

    impl Transport for Fake {
        fn get(&self, url: &str, ua: &str, _: usize) -> Result<Response, NetError> {
            assert!(url.starts_with("https://"));
            assert!(ua.starts_with("SirTone/"));
            self.calls.fetch_add(1, Ordering::Relaxed);
            (self.answer)()
        }
    }

    struct Clock {
        t: Arc<AtomicI64>,
        slept: Arc<AtomicI64>,
    }

    fn net(
        answer: impl Fn() -> Result<Response, NetError> + Send + Sync + 'static,
    ) -> (Net, Arc<AtomicUsize>, Clock) {
        let calls = Arc::new(AtomicUsize::new(0));
        let t = Arc::new(AtomicI64::new(1_000_000));
        let slept = Arc::new(AtomicI64::new(0));
        let (t2, t3, s2) = (t.clone(), t.clone(), slept.clone());
        let n = Net::new(
            Box::new(Fake {
                calls: calls.clone(),
                answer: Box::new(answer),
            }),
            "SirTone/0.1 ( https://github.com/eropermakov/SirTone )".into(),
        )
        .with_clock(
            move || t2.load(Ordering::Relaxed),
            move |d| {
                // Sleeping moves the fake clock.
                t3.fetch_add(d.as_millis() as i64, Ordering::Relaxed);
                s2.fetch_add(d.as_millis() as i64, Ordering::Relaxed);
            },
        );
        (n, calls, Clock { t, slept })
    }

    fn ok() -> Result<Response, NetError> {
        Ok(Response {
            status: 200,
            body: b"{}".to_vec(),
        })
    }

    /// T3.1 acceptance: the offline switch blocks every request to every host.
    #[test]
    fn offline_mode_blocks_all_requests() {
        let (n, calls, _) = net(ok);
        n.set_offline(true);
        for h in Host::ALL {
            assert_eq!(n.get(h, "x"), Err(NetError::Offline));
        }
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        n.set_offline(false);
        assert!(n.get(Host::MusicBrainz, "x").is_ok());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn rate_limits_per_host() {
        let (n, calls, clock) = net(ok);
        for _ in 0..5 {
            n.get(Host::MusicBrainz, "recording/x").unwrap();
        }
        // 0.8 req/s: four waits of 1.25 s.
        assert_eq!(clock.slept.load(Ordering::Relaxed), 4 * 1250);
        // Another host has its own slot.
        let before = clock.slept.load(Ordering::Relaxed);
        n.get(Host::CoverArt, "release/x").unwrap();
        assert_eq!(clock.slept.load(Ordering::Relaxed), before);
        assert_eq!(calls.load(Ordering::Relaxed), 6);
        let _ = clock.t;
    }

    #[test]
    fn three_network_failures_mean_offline_until_the_probe() {
        let (n, calls, clock) = net(|| Err(NetError::Network("down".into())));
        for _ in 0..3 {
            assert!(matches!(
                n.get(Host::OpenMeteo, "forecast"),
                Err(NetError::Network(_))
            ));
        }
        assert!(n.is_offline());
        assert_eq!(n.get(Host::OpenMeteo, "forecast"), Err(NetError::Offline));
        let made = calls.load(Ordering::Relaxed);
        clock.t.fetch_add(61_000, Ordering::Relaxed);
        assert!(!n.is_offline(), "the probe may go");
        let _ = n.get(Host::OpenMeteo, "forecast");
        assert_eq!(calls.load(Ordering::Relaxed), made + 1);
    }

    #[test]
    fn only_paths_under_the_allowed_base() {
        let (n, calls, _) = net(ok);
        for bad in ["https://evil.example/x", "/etc", "../x", "x/../../y"] {
            assert!(matches!(
                n.get(Host::AcoustId, bad),
                Err(NetError::NotAllowed(_))
            ));
        }
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        for h in Host::ALL {
            assert!(h.policy().base.starts_with("https://"));
        }
    }

    struct Mem(Mutex<Vec<(String, Vec<u8>, i64)>>);

    impl HttpCache for Mem {
        fn get(&self, key: &str, now: i64) -> Option<Vec<u8>> {
            let m = self.0.lock().unwrap();
            m.iter()
                .find(|(k, _, until)| k == key && *until > now)
                .map(|(_, b, _)| b.clone())
        }
        fn put(&self, key: &str, body: &[u8], now: i64, ttl: i64) {
            self.0
                .lock()
                .unwrap()
                .push((key.into(), body.to_vec(), now + ttl * 1000));
        }
    }

    #[test]
    fn fresh_answers_come_from_the_cache_even_offline() {
        let (n, calls, clock) = net(ok);
        let n = n.with_cache(Box::new(Mem(Mutex::new(Vec::new()))));
        n.get(Host::Geocoding, "search?name=x").unwrap();
        n.set_offline(true);
        assert_eq!(n.get(Host::Geocoding, "search?name=x").unwrap(), b"{}");
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        // Expired (geocoding: 365 days) → the network, which is off.
        clock.t.fetch_add(366 * DAY * 1000, Ordering::Relaxed);
        assert_eq!(
            n.get(Host::Geocoding, "search?name=x"),
            Err(NetError::Offline)
        );
    }

    #[test]
    fn errors_map_to_app_codes() {
        let e: AppError = NetError::Status(503).into();
        assert_eq!(e.code, ErrorCode::RateLimited);
        assert!(e.retryable);
        let e: AppError = NetError::Offline.into();
        assert_eq!(e.code, ErrorCode::NetworkOffline);
    }
}
