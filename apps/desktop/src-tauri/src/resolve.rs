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
    /// Where the last run stopped: the next one goes on from there (round robin), so a long
    /// «Неизвестные» is worked through, not its first 200 again and again.
    cursor: AtomicI64,
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
                // «Для всех»: ListenBrainz popularity, monthly per track (T3.32).
                crate::popularity::kick(&app);
                // Covers from the Cover Art Archive for albums without one (T3.3).
                crate::covers_online::kick(&app);
                std::thread::sleep(EVERY);
            }
        })
        .ok();
}

/// «Повторить онлайн»: from the first unknown track, now. `false` — offline (the UI says so).
pub fn retry(app: &AppHandle) -> bool {
    if crate::net::offline(app) {
        return false;
    }
    if let Some(r) = app.try_state::<Resolver>() {
        r.cursor.store(0, Ordering::Relaxed);
        r.not_before.store(0, Ordering::Relaxed);
    }
    kick(app);
    true
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
            // Albums placed just now may know their release group: their covers.
            if moved > 0 {
                crate::covers_online::kick(&app);
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
        // `assign` splits artists at commas: a credit with a comma of its own («Earth, Wind &
        // Fire») keeps the artists the library already links.
        let artists = if m.artists.iter().any(|a| a.contains(',')) {
            r.artists_joined.clone()
        } else {
            m.artists.join(", ")
        };
        let assignment = sirtone_library::relocate::Assignment {
            title: m.title.clone(),
            artists,
            album: m.album.clone(),
            track_no: m.album.as_ref().and(m.track_no),
            year,
        };
        return match lib.assign_online(
            r.id,
            assignment,
            m.mbid.clone(),
            m.confidence,
            m.release_group.clone(),
        ) {
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

/// What a visit says about the run.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Visit {
    /// Looked up (counts toward the cap).
    Looked,
    /// Left for another time (playing, or gone from the batch).
    Skipped,
    /// Offline or the service is busy: this one is first next time.
    Stop,
}

/// One round-robin pass over the unknowns (ids ascending), starting after `start`, wrapping to
/// the beginning once and ending where it started; at most `cap` lookups. Returns where the
/// next run goes on from.
fn walk(
    start: i64,
    cap: usize,
    mut rows_after: impl FnMut(i64) -> Vec<i64>,
    mut visit: impl FnMut(i64) -> Visit,
) -> i64 {
    let mut after = start;
    let mut wrapped = start == 0;
    let mut looked = 0;
    loop {
        let rows = rows_after(after);
        if rows.is_empty() {
            if wrapped {
                return 0;
            }
            wrapped = true;
            after = 0;
            continue;
        }
        for id in rows {
            if (wrapped && start > 0 && id > start) || looked >= cap {
                return after;
            }
            match visit(id) {
                Visit::Looked => looked += 1,
                Visit::Skipped => {}
                Visit::Stop => return id - 1,
            }
            after = id;
        }
    }
}

fn run(app: &AppHandle, me: &Resolver) -> (usize, usize) {
    let (mut looked, mut moved) = (0, 0);
    let batch: std::cell::RefCell<Vec<Resolvable>> = std::cell::RefCell::default();
    let cursor = walk(
        me.cursor.load(Ordering::Relaxed),
        RUN_CAP,
        |after| {
            let rows: Vec<Resolvable> = app
                .try_state::<Library>()
                .and_then(|lib| lib.with_read(|c| online::resolvable(c, after, 50)).ok())
                .unwrap_or_default();
            let ids = rows.iter().map(|r| r.id).collect();
            *batch.borrow_mut() = rows;
            ids
        },
        |id| {
            let Some(r) = batch.borrow().iter().find(|r| r.id == id).cloned() else {
                return Visit::Skipped;
            };
            // The playing track is not moved under the engine (queued ones take their new
            // paths after the move).
            let playing = app
                .try_state::<crate::playback::Playback>()
                .and_then(|p| p.current_track_id());
            if playing.is_some() && u32::try_from(id).ok() == playing {
                return Visit::Skipped;
            }
            looked += 1;
            match resolve(app, me, &r) {
                Ok(true) => {
                    moved += 1;
                    Visit::Looked
                }
                Ok(false) | Err(Stop::Track) => Visit::Looked,
                Err(Stop::Run) => Visit::Stop,
            }
        },
    );
    me.cursor.store(cursor, Ordering::Relaxed);
    (looked, moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(all: &[i64]) -> impl FnMut(i64) -> Vec<i64> + '_ {
        |after| {
            all.iter()
                .copied()
                .filter(|&i| i > after)
                .take(50)
                .collect()
        }
    }

    /// 250 unknowns, 200 per run: the second run reaches the last ones, then wraps around.
    #[test]
    fn runs_go_round_the_whole_list() {
        let all: Vec<i64> = (1..=250).collect();
        let mut seen = Vec::new();
        let c1 = walk(0, 200, rows(&all), |id| {
            seen.push(id);
            Visit::Looked
        });
        assert_eq!((seen.len(), c1), (200, 200));
        seen.clear();
        let c2 = walk(c1, 200, rows(&all), |id| {
            seen.push(id);
            Visit::Looked
        });
        assert_eq!(seen[..50], (201..=250).collect::<Vec<_>>()[..]);
        assert_eq!(seen[50], 1);
        assert_eq!(seen.len(), 200);
        assert_eq!(c2, 150);
        // A busy service: that track is first next time; skipped ones do not count.
        let c3 = walk(0, 200, rows(&all), |id| match id {
            1..=9 => Visit::Skipped,
            10..=11 => Visit::Looked,
            _ => Visit::Stop,
        });
        assert_eq!(c3, 11);
        // A short list ends where it started.
        let few: Vec<i64> = vec![3, 7, 9];
        let mut v = Vec::new();
        let c4 = walk(7, 200, rows(&few), |id| {
            v.push(id);
            Visit::Looked
        });
        assert_eq!((v, c4), (vec![9, 3, 7], 7));
    }
}
