//! `resolve_unknown` (FR-REC-006, `05` §4.7, 2.0: D-84, D-87): when the network is there, the
//! «Неизвестные» are looked up on MusicBrainz by what the library knows (no key, no account);
//! a confirmed match moves the file to its place (import worker, journal), weaker answers stay
//! as hints. Runs on its own thread at the MusicBrainz rate (≤ 0.8 req/s, `sirtone-net`); a
//! busy service (503/429) stops the run for a while. Triggers: after start, after an import,
//! when «Работать полностью офлайн» is turned off, and every 30 minutes.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use sirtone_library::online::{self, Resolvable};
use sirtone_net::{Host, NetError};
use sirtone_recognize::online::{
    Hints, Match, Verdict, decide, release_group_path, release_group_year, search_path,
};
use tauri::{AppHandle, Manager};

use crate::library::Library;
use crate::net::NetState;

/// Tracks looked up per run (a large «Неизвестные» is worked through over several runs).
const RUN_CAP: usize = 200;
/// After 503/429: wait before the next run.
const BUSY_MS: i64 = 10 * 60 * 1000;
const EVERY: Duration = Duration::from_secs(30 * 60);
const FIRST: Duration = Duration::from_secs(90);

#[derive(Default)]
pub struct Resolver {
    running: AtomicBool,
    not_before: AtomicI64,
}

fn now_ms() -> i64 {
    sirtone_library::db::now_ms()
}

/// The periodic trigger (after start, then every 30 minutes).
pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("sirtone-resolve-timer".into())
        .spawn(move || {
            std::thread::sleep(FIRST);
            loop {
                kick(&app);
                std::thread::sleep(EVERY);
            }
        })
        .ok();
}

/// Starts a run unless one is going, the network is off or the service asked to wait.
pub fn kick(app: &AppHandle) {
    let Some(r) = app.try_state::<Resolver>() else {
        return;
    };
    if crate::net::offline(app) || now_ms() < r.not_before.load(Ordering::Relaxed) {
        return;
    }
    if r.running.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sirtone-resolve".into())
        .spawn(move || {
            sirtone_platform::power::background_thread();
            let me = app.state::<Resolver>();
            let (looked, moved) = run(&app, &me);
            if looked > 0 {
                tracing::info!(target: "sirtone::resolve", looked, moved, "online lookup");
            }
            me.running.store(false, Ordering::Release);
        });
    if spawned.is_err() {
        r.running.store(false, Ordering::Release);
    }
}

enum Stop {
    /// Offline or the service is busy: the run ends.
    Run,
    /// This track only.
    Track,
}

fn fetch(app: &AppHandle, me: &Resolver, path: &str) -> Result<Vec<u8>, Stop> {
    let net = &app.state::<NetState>().0;
    match net.get(Host::MusicBrainz, path) {
        Ok(b) => Ok(b),
        Err(NetError::Status(429 | 503)) => {
            me.not_before.store(now_ms() + BUSY_MS, Ordering::Relaxed);
            Err(Stop::Run)
        }
        Err(NetError::Offline | NetError::Network(_)) => Err(Stop::Run),
        Err(_) => Err(Stop::Track),
    }
}

/// The ways to read a track: what the library holds, and — for a tags/file-name conflict —
/// the file name too (`_Unknown/` keeps the original name).
fn hints_of(r: &Resolvable) -> Vec<Hints> {
    let mut out = Vec::new();
    if let Some(artist) = &r.artist {
        out.push(Hints {
            title: r.title.clone(),
            artist: artist.clone(),
            album: r.album.clone(),
            duration_ms: r.duration_ms,
        });
    }
    if r.reason.as_deref() == Some("conflict") {
        let name = r.rel_path.rsplit('/').next().unwrap_or(&r.rel_path);
        let g = sirtone_recognize::name::parse_file_name(name, |_| false);
        if let (Some(artist), Some(title)) = (g.artist, g.title) {
            out.push(Hints {
                title,
                artist,
                album: None,
                duration_ms: r.duration_ms,
            });
        }
    }
    out
}

/// One track: `Ok(true)` — moved into place.
fn resolve(app: &AppHandle, me: &Resolver, r: &Resolvable) -> Result<bool, Stop> {
    let readings = hints_of(r);
    if readings.is_empty() {
        return Ok(false);
    }
    let mut confirmed: Vec<Match> = Vec::new();
    let mut hints = Vec::new();
    for h in &readings {
        let body = fetch(app, me, &search_path(h, sirtone_net::encode))?;
        match decide(&body, h) {
            Ok(Verdict::Confirmed(m)) => confirmed.push(m),
            Ok(Verdict::Hint(c)) => hints.extend(c),
            Ok(Verdict::Nothing) | Err(_) => {}
        }
    }
    let lib = app.state::<Library>();
    // A conflict is settled only when exactly one reading is confirmed.
    if let [m] = confirmed.as_slice() {
        let year = match m.release_group.as_deref().and_then(release_group_path) {
            Some(p) => fetch(app, me, &p).ok().and_then(|b| release_group_year(&b)),
            None => None,
        };
        let assignment = sirtone_library::relocate::Assignment {
            title: m.title.clone(),
            artists: m.artists.join(", "),
            album: m.album.clone(),
            track_no: m.album.as_ref().and(m.track_no),
            year,
        };
        return match lib.assign_online(r.id, assignment, m.mbid.clone(), m.confidence) {
            Ok(_) => Ok(true),
            Err(e) => {
                tracing::warn!(target: "sirtone::resolve", track = r.id, code = ?e.code, "move failed");
                Ok(false)
            }
        };
    }
    for m in &confirmed {
        hints.push(sirtone_recognize::online::Candidate {
            mbid: m.mbid.clone(),
            title: m.title.clone(),
            artists: m.artists.clone(),
            length_ms: None,
            d: 1.0 - m.confidence,
        });
    }
    let json = (!hints.is_empty()).then(|| {
        serde_json::Value::Array(
            hints
                .iter()
                .take(3)
                .map(|c| {
                    serde_json::json!({
                        "source": "musicbrainz",
                        "mbid": c.mbid,
                        "title": c.title,
                        "artists": c.artists,
                        "lengthMs": c.length_ms,
                        "d": (c.d * 1000.0).round() / 1000.0,
                    })
                })
                .collect(),
        )
        .to_string()
    });
    let _ = lib.with_write(|c| online::set_candidates(c, r.id, json.as_deref()));
    Ok(false)
}

fn run(app: &AppHandle, me: &Resolver) -> (usize, usize) {
    let (mut looked, mut moved, mut after) = (0, 0, 0);
    while looked < RUN_CAP {
        let Some(lib) = app.try_state::<Library>() else {
            break;
        };
        let rows = match lib.with_read(|c| online::resolvable(c, after, 50)) {
            Ok(r) if !r.is_empty() => r,
            _ => break,
        };
        for r in &rows {
            after = r.id;
            if looked >= RUN_CAP {
                break;
            }
            looked += 1;
            match resolve(app, me, r) {
                Ok(true) => moved += 1,
                Ok(false) | Err(Stop::Track) => {}
                Err(Stop::Run) => return (looked, moved),
            }
        }
    }
    (looked, moved)
}
