//! Covers from the Cover Art Archive (T3.3, FR-REC-009 «встроенная → файл в папке → CAA →
//! заглушка», `05` §4.6): albums without a cover that know their MusicBrainz release or release
//! group (Picard tags, the online lookup D-87) get the front cover, 500 px. Only when there is a
//! network; ≤ 4 req/s and 90 days of cache in `sirtone-net`; a cover the user chose is never
//! replaced (`cover_locked`). An album asked once in a session is not asked again until restart.
//! Albums that stay without a cover are then searched on Deezer and iTunes by artist and title
//! (D-162, `08` §4): a close match only. Stored with source `caa` (the column means «from the web»).

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use sirtone_core::ports::{CoverSource, valid_mbid};
use sirtone_library::covers;
use sirtone_net::{Host, NetError, encode};
use sirtone_recognize::web_covers;
use tauri::{AppHandle, Manager};

use crate::library::Library;
use crate::net::NetState;

/// Albums per run.
const RUN: u32 = 200;

#[derive(Default)]
pub struct CoversOnline {
    running: AtomicBool,
    asked: Mutex<HashSet<i64>>,
    asked_web: Mutex<HashSet<i64>>,
}

/// «Найти обложки» (D-162): asks every album without a cover again, now. `false` — offline.
#[tauri::command]
#[specta::specta]
pub fn covers_find_online(app: AppHandle) -> bool {
    if crate::net::offline(&app) {
        return false;
    }
    if let Some(c) = app.try_state::<CoversOnline>() {
        if let Ok(mut a) = c.asked.lock() {
            a.clear();
        }
        if let Ok(mut a) = c.asked_web.lock() {
            a.clear();
        }
    }
    kick(&app);
    true
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
            let n = run(&app, &me) + run_web(&app, &me);
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
        // Asked again next run unless an answer came (a cover, or «no art» for every edition).
        let forget = || {
            if let Ok(mut a) = me.asked.lock() {
                a.remove(&album);
            }
        };
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
                Err(NetError::Offline | NetError::Network(_)) => {
                    // No network right now (a laptop just woke): this album and the rest wait
                    // for the next run, not for a restart.
                    forget();
                    return got;
                }
                Err(NetError::Status(404)) => {}
                // 429/503 and the like: next run.
                Err(_) => {
                    forget();
                    break;
                }
            }
        }
    }
    got
}

/// Albums still without a cover: Deezer first, then iTunes. Closed matches only.
fn run_web(app: &AppHandle, me: &CoversOnline) -> usize {
    let lib = app.state::<Library>();
    let Some(root) = lib.root() else {
        return 0;
    };
    let Ok(albums) = lib.with_read(|c| sirtone_library::online::albums_for_web_cover(c, RUN))
    else {
        return 0;
    };
    let net = &app.state::<NetState>().0;
    let mut got = 0;
    for (album, title, artist) in albums {
        if artist.trim().is_empty()
            || !me
                .asked_web
                .lock()
                .map(|mut a| a.insert(album))
                .unwrap_or(false)
        {
            continue;
        }
        let image = |host_search: Host,
                     search: String,
                     pick: fn(&[u8], &str, &str) -> Option<String>,
                     host_image: Host|
         -> Result<Option<Vec<u8>>, NetError> {
            let json = match net.get(host_search, &search) {
                Ok(j) => j,
                Err(NetError::Status(_)) => return Ok(None),
                Err(e) => return Err(e),
            };
            match pick(&json, &artist, &title) {
                Some(path) => match net.get(host_image, &path) {
                    Ok(b) => Ok(Some(b)),
                    Err(NetError::Status(_)) => Ok(None),
                    Err(e) => Err(e),
                },
                None => Ok(None),
            }
        };
        let found = image(
            Host::Deezer,
            web_covers::deezer_path(&artist, &title, encode),
            web_covers::deezer_pick,
            Host::DeezerImages,
        )
        .and_then(|d| match d {
            Some(b) => Ok(Some(b)),
            None => image(
                Host::Itunes,
                web_covers::itunes_path(&artist, &title, encode),
                web_covers::itunes_pick,
                Host::AppleImages,
            ),
        });
        match found {
            Ok(Some(bytes)) => {
                let stored = lib.with_write(|c| {
                    let id = covers::store(c, &root, &bytes, CoverSource::Caa)?;
                    covers::link_album(c, album, id)
                });
                if stored.is_ok() {
                    got += 1;
                }
            }
            Ok(None) => {}
            Err(_) => {
                // Offline, busy or an odd answer: this album waits for the next run.
                if let Ok(mut a) = me.asked_web.lock() {
                    a.remove(&album);
                }
                return got;
            }
        }
    }
    got
}
