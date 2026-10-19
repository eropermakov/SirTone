//! The network layer in the app (T3.1, `03` §11): one `sirtone_net::Net` for every outgoing
//! request — the «Работать полностью офлайн» switch (`network.offline_mode`) blocks them all,
//! the updater included; responses are cached in the library's `http_cache`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sirtone_core::ports::HttpCache;
use sirtone_net::{Net, NetError, Reqwest, Response, Transport};
use tauri::{AppHandle, Manager, State};

use crate::library::{Library, db_path, medium};
use crate::settings_store::SettingsStore;

/// The shared network state.
pub struct NetState(pub Arc<Net>);

/// reqwest's blocking client is built on first use, on the calling (background) thread — never
/// inside the async runtime.
struct Lazy(OnceLock<Result<Reqwest, NetError>>);

impl Transport for Lazy {
    fn get(&self, url: &str, ua: &str, max: usize) -> Result<Response, NetError> {
        match self.0.get_or_init(Reqwest::new) {
            Ok(r) => r.get(url, ua, max),
            Err(e) => Err(e.clone()),
        }
    }

    fn post(&self, url: &str, ua: &str, body: &[u8], max: usize) -> Result<Response, NetError> {
        match self.0.get_or_init(Reqwest::new) {
            Ok(r) => r.post(url, ua, body, max),
            Err(e) => Err(e.clone()),
        }
    }
}

/// `http_cache` of the current library, on its own connection.
struct DbCache {
    app: AppHandle,
    conn: Mutex<Option<(PathBuf, Connection)>>,
}

impl DbCache {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Option<T> {
        let root = self.app.try_state::<Library>()?.root()?;
        let mut slot = self.conn.lock().ok()?;
        if slot.as_ref().is_none_or(|(r, _)| *r != root) {
            let c = sirtone_library::open_existing(&db_path(&root), medium(&root)).ok()?;
            *slot = Some((root, c));
        }
        let (_, c) = slot.as_ref()?;
        f(c).ok()
    }
}

impl HttpCache for DbCache {
    fn get(&self, key: &str, now_ms: i64) -> Option<Vec<u8>> {
        self.with(|c| {
            c.query_row(
                "SELECT body FROM http_cache WHERE key = ?1 AND fetched_at + ttl_s * 1000 > ?2",
                params![key, now_ms],
                |r| r.get(0),
            )
            .optional()
        })
        .flatten()
    }

    fn put(&self, key: &str, body: &[u8], now_ms: i64, ttl_s: i64) {
        let _ = self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO http_cache(key, body, fetched_at, ttl_s) VALUES (?1, ?2, ?3, ?4)",
                params![key, body, now_ms, ttl_s],
            )
        });
    }
}

pub fn start(app: &AppHandle) -> NetState {
    let ua = format!(
        "SirTone/{} ( https://github.com/eropermakov/SirTone )",
        env!("CARGO_PKG_VERSION")
    );
    // Isolated test instances (smoke) may answer from recorded files — nothing leaves the PC.
    let transport: Box<dyn Transport> = match (
        std::env::var_os("SIRTONE_NET_FIXTURES"),
        std::env::var_os("SIRTONE_DATA_DIR"),
    ) {
        (Some(dir), Some(_)) => Box::new(sirtone_net::Fixtures::new(PathBuf::from(dir))),
        _ => Box::new(Lazy(OnceLock::new())),
    };
    let net = Net::new(transport, ua).with_cache(Box::new(DbCache {
        app: app.clone(),
        conn: Mutex::new(None),
    }));
    let state = NetState(Arc::new(net));
    state.0.set_offline(forced(app));
    state
}

fn forced(app: &AppHandle) -> bool {
    app.try_state::<Arc<SettingsStore>>()
        .and_then(|s| s.get()["network"]["offline_mode"].as_bool())
        .unwrap_or(false)
}

/// The settings changed (`network`): the switch applies at once.
pub fn apply(app: &AppHandle) {
    if let Some(n) = app.try_state::<NetState>() {
        n.0.set_offline(forced(app));
    }
    // Back online: the «Неизвестные» are looked up (FR-REC-006).
    crate::resolve::kick(app);
}

/// Offline now — the switch, or the network down (the updater asks this first).
pub fn offline(app: &AppHandle) -> bool {
    app.try_state::<NetState>()
        .is_some_and(|n| n.0.is_offline())
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NetStatus {
    /// No request goes out now.
    pub offline: bool,
    /// Because of the «Работать полностью офлайн» switch.
    pub forced: bool,
}

#[tauri::command]
#[specta::specta]
pub fn net_status(net: State<'_, NetState>) -> NetStatus {
    NetStatus {
        offline: net.0.is_offline(),
        forced: net.0.forced_offline(),
    }
}
