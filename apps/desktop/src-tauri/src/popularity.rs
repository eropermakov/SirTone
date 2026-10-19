//! `refresh_popularity` (T3.32, FR-ALL-002, `14` §16): once a month, the ListenBrainz popularity
//! of tracks with a recording MBID (Picard tags or the online lookup) goes into
//! `popularity_cache` for «Для всех». ≤ 1 request/s (`sirtone-net`), 100 MBIDs per request, a
//! busy service (429/503) pauses it; offline — nothing happens, «Для всех» says it has no
//! popularity data.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use sirtone_library::popularity::{BATCH, REFRESH_MS, request, stale, store};
use sirtone_net::{Host, NetError};
use tauri::{AppHandle, Manager};

use crate::library::Library;
use crate::net::NetState;

/// MBIDs refreshed per run.
const RUN: u32 = 2_000;
const BUSY_MS: i64 = 30 * 60 * 1000;

#[derive(Default)]
pub struct Popularity {
    running: AtomicBool,
    not_before: AtomicI64,
}

/// Starts a refresh unless one is going, the network is off or the service asked to wait.
pub fn kick(app: &AppHandle) {
    let Some(p) = app.try_state::<Popularity>() else {
        return;
    };
    let now = sirtone_library::db::now_ms();
    if crate::net::offline(app) || now < p.not_before.load(Ordering::Relaxed) {
        return;
    }
    if p.running.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sirtone-popularity".into())
        .spawn(move || {
            sirtone_platform::power::background_thread();
            let me = app.state::<Popularity>();
            let n = run(&app, &me);
            if n > 0 {
                tracing::info!(target: "sirtone::popularity", stored = n, "popularity refreshed");
            }
            me.running.store(false, Ordering::Release);
        });
    if spawned.is_err() {
        p.running.store(false, Ordering::Release);
    }
}

fn run(app: &AppHandle, me: &Popularity) -> usize {
    let now = sirtone_library::db::now_ms();
    let lib = app.state::<Library>();
    let Ok(mbids) = lib.with_read(|c| stale(c, now - REFRESH_MS, RUN)) else {
        return 0;
    };
    let net = &app.state::<NetState>().0;
    let mut stored = 0;
    for batch in mbids.chunks(BATCH) {
        match net.post(Host::ListenBrainz, "popularity/recording", &request(batch)) {
            Ok(answer) => {
                stored += lib
                    .with_write(|c| store(c, &answer, sirtone_library::db::now_ms()))
                    .unwrap_or(0);
            }
            Err(NetError::Status(429 | 503)) => {
                me.not_before
                    .store(sirtone_library::db::now_ms() + BUSY_MS, Ordering::Relaxed);
                break;
            }
            Err(NetError::Offline | NetError::Network(_)) => break,
            Err(e) => {
                tracing::warn!(target: "sirtone::popularity", error = %e, "batch refused");
            }
        }
    }
    stored
}
