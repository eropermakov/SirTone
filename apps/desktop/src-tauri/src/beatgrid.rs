//! Beat grids for the automix L3 (T4.9c, D-100), lazily: when a track is handed over, the first
//! and last 30 s of the two tracks are decoded in the background, gridded
//! (`sirtone_analysis::grid`, tempo hint from B1) and kept in memory; the player then re-plans the
//! handover (the engine takes a new plan until the overlap begins). No library re-analysis.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use sirtone_playback::automix::BeatGrid;
use tauri::{AppHandle, Manager};

const WINDOW_MS: u64 = 30_000;
/// How many tracks' grids are kept.
const CACHE: usize = 64;

/// A track's head and tail grids (`None` inside: measured, not usable).
pub type Edges = (Option<BeatGrid>, Option<BeatGrid>);

#[derive(Default)]
pub struct Grids {
    cache: Mutex<HashMap<u32, Edges>>,
    pending: Mutex<HashSet<u32>>,
}

impl Grids {
    /// The grids of `track` if measured; otherwise measuring starts (once) in the background and
    /// the handover is re-planned when it is done.
    pub fn get_or_request(
        &self,
        app: &AppHandle,
        track: u32,
        path: &Path,
        bpm: Option<f32>,
    ) -> Option<Edges> {
        if let Some(e) = self
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&track)
        {
            return Some(*e);
        }
        let hint = bpm.filter(|b| *b > 0.0)?;
        if !self
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(track)
        {
            return None;
        }
        let (app, path) = (app.clone(), path.to_path_buf());
        let _ = std::thread::Builder::new()
            .name("sirtone-grid".into())
            .spawn(move || {
                sirtone_platform::power::background_thread();
                let edges = measure(&path, f64::from(hint)).unwrap_or((None, None));
                let grids = app.state::<Grids>();
                {
                    let mut c = grids.cache.lock().unwrap_or_else(|p| p.into_inner());
                    if c.len() >= CACHE {
                        c.clear();
                    }
                    c.insert(track, edges);
                }
                grids
                    .pending
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&track);
                if let Some(pb) = app.try_state::<crate::playback::Playback>() {
                    pb.replan();
                }
            });
        None
    }
}

/// Mono samples of `[from_ms, from_ms + WINDOW_MS)` and the rate.
fn window(path: &Path, from_ms: u64) -> Option<(Vec<f32>, u32)> {
    let mut src = sirtone_audio::decode::Source::open(path).ok()?;
    if from_ms > 0 {
        src.seek(from_ms).ok()?;
    }
    let (rate, ch) = (src.rate, src.channels.max(1));
    let want = (u64::from(rate) * WINDOW_MS / 1000) as usize;
    let mut mono = Vec::with_capacity(want);
    let mut buf = Vec::new();
    while mono.len() < want {
        buf.clear();
        if !src.next_into(&mut buf).ok()? {
            break;
        }
        for f in buf.chunks(ch) {
            mono.push(f.iter().sum::<f32>() / ch as f32);
        }
    }
    mono.truncate(want);
    Some((mono, rate))
}

fn grid_at(path: &Path, from_ms: u64, hint: f64) -> Option<BeatGrid> {
    let (mono, rate) = window(path, from_ms)?;
    let g = sirtone_analysis::grid::estimate(&mono, rate, hint)?;
    Some(BeatGrid {
        bpm: g.bpm as f32,
        beat_ms: from_ms as f64 + g.first_beat_s * 1000.0,
        stability: g.stability as f32,
    })
}

fn measure(path: &Path, hint: f64) -> Option<Edges> {
    let dur = sirtone_audio::decode::Source::open(path)
        .ok()?
        .duration_ms?;
    let tail_from = dur.saturating_sub(WINDOW_MS);
    Some((grid_at(path, 0, hint), grid_at(path, tail_from, hint)))
}
