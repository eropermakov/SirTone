//! «Моя волна» in the app (Stage 3, docs/14 §2.2): the engine lives on its own `sirtone-wave`
//! thread. `wave_start` builds it from the library (statistics, likes, «никогда», «не сейчас»)
//! and plays the first tracks; the playback sink asks for more when fewer than two are ahead,
//! and every refill reloads the catalog — the plays, skips and likes since then count at once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::wave::Clock;
use sirtone_platform::clock::SystemClock;
use sirtone_wave::context::ContextBuilder;
use sirtone_wave::engine::{Config, Engine};
use tauri::{AppHandle, Manager, State};

use crate::library::Library;
use crate::playback::{PlayContext, Playback, items_for};
use crate::settings_store::SettingsStore;

/// Tracks queued when the wave starts, and per refill (docs/14 §7: 3–5 ahead).
const FIRST: usize = 5;
const MORE: usize = 3;

enum Cmd {
    Start(Sender<Result<u32, AppError>>),
    More,
}

pub struct Wave {
    tx: Sender<Cmd>,
    /// A refill is on its way (the sink asks 4 times a second).
    pending: std::sync::Arc<AtomicBool>,
}

impl Wave {
    pub fn start(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        let pending = std::sync::Arc::new(AtomicBool::new(false));
        let worker_pending = pending.clone();
        std::thread::Builder::new()
            .name("sirtone-wave".into())
            .spawn(move || worker(&app, &rx, &worker_pending))
            .ok();
        Self { tx, pending }
    }

    /// More tracks for a wave that is running out (from the playback sink; never blocks).
    pub fn request_more(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(Cmd::More);
        }
    }
}

/// Slider position 0…1 from the settings (`wave.novelty`, 0–100).
fn novelty(app: &AppHandle) -> f32 {
    app.try_state::<std::sync::Arc<SettingsStore>>()
        .and_then(|s| s.get()["wave"]["novelty"].as_u64())
        .map_or(0.5, |v| (v.min(100) as f32) / 100.0)
}

/// The wave thread's own connection to the current library: reading the catalog on a refill
/// must not hold the shared read connection the lists use (≤ 100 ms rule).
type Conn = Option<(std::path::PathBuf, rusqlite::Connection)>;

fn conn<'a>(app: &AppHandle, slot: &'a mut Conn) -> Result<&'a rusqlite::Connection, AppError> {
    let root = app
        .state::<Library>()
        .root()
        .ok_or_else(|| AppError::new(ErrorCode::LibraryRootNotSet))?;
    if slot.as_ref().is_none_or(|(r, _)| *r != root) {
        let c = sirtone_library::open_existing(
            &crate::library::db_path(&root),
            crate::library::medium(&root),
        )?;
        *slot = Some((root, c));
    }
    slot.as_ref()
        .map(|(_, c)| c)
        .ok_or_else(|| AppError::new(ErrorCode::Internal))
}

type Read = (
    Vec<sirtone_core::wave::WaveTrack>,
    Vec<sirtone_core::wave::NotNowRow>,
);

fn read(app: &AppHandle, slot: &mut Conn, now: i64) -> Result<Read, AppError> {
    let c = conn(app, slot)?;
    Ok((
        sirtone_library::wave::catalog(c)?,
        sirtone_library::wave::not_now(c, now)?,
    ))
}

/// The next `n` library tracks from the engine, built or refreshed from the library.
fn next_ids(
    app: &AppHandle,
    slot: &mut Conn,
    engine: &mut Option<Engine>,
    n: usize,
) -> Result<Vec<u32>, AppError> {
    let clock = SystemClock;
    let now = clock.now_utc_ms();
    let (tracks, shelf) = match read(app, slot, now) {
        Ok(v) => v,
        // A stale handle (the stick came back): reopen once.
        Err(_) => {
            *slot = None;
            read(app, slot, now)?
        }
    };
    match engine {
        Some(e) => e.reload(&tracks, now),
        None => {
            let cfg = Config::builtin()
                .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e.to_string()))?;
            *engine = Some(Engine::new(cfg, &tracks, now, now as u64 | 1));
        }
    }
    let Some(e) = engine.as_mut() else {
        return Ok(Vec::new());
    };
    let ctx = ContextBuilder::default().snapshot(&clock);
    Ok(e.next(&ctx, &shelf, novelty(app), n, now)
        .into_iter()
        .filter_map(|id| u32::try_from(id).ok())
        .collect())
}

fn worker(app: &AppHandle, rx: &Receiver<Cmd>, pending: &AtomicBool) {
    let mut engine: Option<Engine> = None;
    let mut slot: Conn = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start(reply) => {
                // A new wave: a new session (spacing and group counts start over).
                engine = None;
                let r = next_ids(app, &mut slot, &mut engine, FIRST).and_then(|ids| {
                    let lib = app.state::<Library>();
                    let items = items_for(&lib, &ids, PlayContext::Wave)?;
                    let n = items.len() as u32;
                    if n > 0 {
                        app.state::<Playback>().play_queue(items, 0);
                    }
                    Ok(n)
                });
                let _ = reply.send(r);
            }
            Cmd::More => {
                if engine.is_some() {
                    match next_ids(app, &mut slot, &mut engine, MORE)
                        .and_then(|ids| items_for(&app.state::<Library>(), &ids, PlayContext::Wave))
                    {
                        Ok(items) if !items.is_empty() => app.state::<Playback>().append(items),
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!(target: "sirtone::wave", code = ?e.code, "refill failed");
                        }
                    }
                }
                pending.store(false, Ordering::Release);
            }
        }
    }
}

/// «Моя волна»: the engine picks and plays (returns how many tracks were queued).
#[tauri::command(async)]
#[specta::specta]
pub fn wave_start(wave: State<'_, Wave>) -> Result<u32, AppError> {
    let (tx, rx) = mpsc::channel();
    wave.tx
        .send(Cmd::Start(tx))
        .map_err(|_| AppError::new(ErrorCode::Internal))?;
    rx.recv_timeout(Duration::from_secs(20))
        .map_err(|_| AppError::new(ErrorCode::Internal).with_details("wave start timed out"))?
}
