//! Covers from the Cover Art Archive (T3.3, FR-REC-009 «встроенная → файл в папке → CAA →
//! заглушка», `05` §4.6): albums without a cover that know their MusicBrainz release or release
//! group (Picard tags, the online lookup D-87) get the front cover, 500 px. Only when there is a
//! network; ≤ 4 req/s and 90 days of cache in `sirtone-net`; a cover the user chose is never
//! replaced (`cover_locked`). An album asked once in a session is not asked again until restart.

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use sirtone_core::ports::{CoverSource, valid_mbid};
use sirtone_library::covers;
use sirtone_net::{Host, NetError};
use tauri::{AppHandle, Manager};

use crate::library::Library;
use crate::net::NetState;

/// Albums per run.
const RUN: u32 = 200;

#[derive(Default)]
pub struct CoversOnline {
    running: AtomicBool,
    asked: Mutex<HashSet<i64>>,
}

pub fn kick(app: &AppHandle) {
    let Some(c) = app.try_state::<CoversOnline>() else {
        return;
    };
    if crate::net::offline(app) || c.running.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("sirtone-caa".into())
        .spawn(move || {
            sirtone_platform::power::background_thread();
            let me = app.state::<CoversOnline>();
            let n = run(&app, &me);
            if n > 0 {
                tracing::info!(target: "sirtone::covers", albums = n, "covers from the Cover Art Archive");
                app.state::<Library>().notify_changed(&app);
            }
            me.running.store(false, Ordering::Release);
        });
    if spawned.is_err() {
        c.running.store(false, Ordering::Release);
    }
}

fn run(app: &AppHandle, me: &CoversOnline) -> usize {
    let lib = app.state::<Library>();
    let Some(root) = lib.root() else {
        return 0;
    };
    let Ok(albums) = lib.with_read(|c| sirtone_library::online::albums_for_caa(c, RUN)) else {
        return 0;
    };
    let net = &app.state::<NetState>().0;
    let mut got = 0;
    for (album, release, group) in albums {
        if !me
            .asked
            .lock()
            .map(|mut a| a.insert(album))
            .unwrap_or(false)
        {
            continue;
        }
        // The release group's front first (any edition of the album), then the release's.
        let paths = [
            group
                .filter(|g| valid_mbid(g))
                .map(|g| format!("release-group/{g}/front-500")),
            release
                .filter(|r| valid_mbid(r))
                .map(|r| format!("release/{r}/front-500")),
        ];
        for path in paths.into_iter().flatten() {
            match net.get(Host::CoverArt, &path) {
                Ok(bytes) => {
                    let stored = lib.with_write(|c| {
                        let id = covers::store(c, &root, &bytes, CoverSource::Caa)?;
                        covers::link_album(c, album, id)
                    });
                    if stored.is_ok() {
                        got += 1;
                    }
                    break;
                }
                Err(NetError::Offline | NetError::Network(_)) => return got,
                // 404: this edition has no art; 429/503: next time.
                Err(_) => {}
            }
        }
    }
    got
}
