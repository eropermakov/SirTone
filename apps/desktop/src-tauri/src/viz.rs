//! The visualiser feed (T4.1, FR-VIZ-001/007, `04` §9, D-14): while a visualiser is on screen,
//! a 60 Hz thread sends frames of what is being heard over a Tauri `Channel` as raw bytes (S-04:
//! no loss, jitter p99 < 1 ms). The position and the beat phase follow the player: the snapshot
//! (4 Hz) is extrapolated between updates; the tempo comes from the track's analysis (B1).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use sirtone_library::palette::{self, Swatch};

use tauri::ipc::{Channel, InvokeResponseBody, IpcResponse};
use tauri::{AppHandle, Manager, State};

use crate::library::Library;
use crate::playback::{Playback, PlaybackState};

const PERIOD: Duration = Duration::from_nanos(16_666_667);

/// One frame (`sirtone_audio::viz::FRAME` bytes) — an `ArrayBuffer` on the page.
pub struct VizFrame(Vec<u8>);

impl IpcResponse for VizFrame {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        Ok(InvokeResponseBody::Raw(self.0))
    }
}

impl specta::Type for VizFrame {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        specta_typescript::Unknown::<()>::definition(types)
    }
}

#[derive(Default)]
pub struct Viz {
    running: Mutex<Option<Arc<AtomicBool>>>,
    /// «Цвет вашей музыки» and when it was worked out (recomputed after `COLLECTION_TTL`).
    collection: Mutex<Option<(Instant, Vec<VizSwatch>)>>,
}

/// The collection colour follows the library slowly: recomputed at most this often.
const COLLECTION_TTL: Duration = Duration::from_secs(600);

/// A dominant colour, OKLCH (`h` in degrees), with its share `w`.
#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
pub struct VizSwatch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
    pub w: f64,
}

impl From<Swatch> for VizSwatch {
    fn from(s: Swatch) -> Self {
        Self {
            l: f64::from(s.l),
            c: f64::from(s.c),
            h: f64::from(s.h),
            w: f64::from(s.w),
        }
    }
}

/// What the «Волны» palette is built from (T4.2, `06` §7.2).
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct VizColors {
    /// The track's vibe, 0…1 (`None` — not analysed yet).
    pub valence: Option<f64>,
    pub arousal: Option<f64>,
    /// The rest of the vibe vector for the motion (T4.5, FR-VIZ-002), 0…1.
    pub energy: Option<f64>,
    pub aggression: Option<f64>,
    pub danceability: Option<f64>,
    pub acousticness: Option<f64>,
    /// The cover's dominant colours, the biggest first (empty — no cover).
    pub cover: Vec<VizSwatch>,
    /// «Цвет вашей музыки» (empty — no covers in the library).
    pub collection: Vec<VizSwatch>,
}

impl Viz {
    fn stop(&self) {
        if let Ok(mut r) = self.running.lock()
            && let Some(flag) = r.take()
        {
            flag.store(false, Ordering::Release);
        }
    }
}

/// A cover's palette: cached in `cover.palette`, else computed from the 64 px variant (decoded
/// with no lock held) and stored.
fn palette_of(app: &AppHandle, root: &std::path::Path, hash: &str) -> Vec<Swatch> {
    let lib = app.state::<Library>();
    if let Ok(Some(p)) = lib.with_read(|c| palette::cached(c, hash)) {
        return p;
    }
    let Ok(p) = palette::of_cover(root, hash) else {
        return Vec::new();
    };
    let _ = lib.with_write(|c| palette::store(c, hash, &p));
    p
}

/// The tempo of a library track (0 — not analysed).
fn bpm_of(app: &AppHandle, track: Option<u32>) -> f32 {
    let Some(t) = track else { return 0.0 };
    app.state::<Library>()
        .with_read(|c| {
            Ok(c.query_row(
                "SELECT coalesce(bpm, 0) FROM track WHERE id = ?1",
                [t],
                |r| r.get::<_, f64>(0),
            )?)
        })
        .map_or(0.0, |b| b as f32)
}

/// Starts the feed (one at a time: a new start replaces the old one).
#[tauri::command]
#[specta::specta]
pub fn viz_start(app: AppHandle, viz: State<'_, Viz>, channel: Channel<VizFrame>) {
    viz.stop();
    let flag = Arc::new(AtomicBool::new(true));
    if let Ok(mut r) = viz.running.lock() {
        *r = Some(flag.clone());
    }
    let mut source = app.state::<Playback>().viz_source();
    let _ = std::thread::Builder::new()
        .name("sirtone-viz".into())
        .spawn(move || {
            let pb = app.state::<Playback>();
            let mut next = Instant::now();
            let mut anchor = (u64::MAX, Instant::now());
            let mut track = None;
            let mut bpm = 0f32;
            while flag.load(Ordering::Acquire) {
                let s = pb.snapshot();
                if s.track_id != track {
                    track = s.track_id;
                    bpm = bpm_of(&app, track);
                }
                // The player reports 4 times a second: extrapolate between reports.
                let pos = u64::from(s.position_ms);
                if pos != anchor.0 {
                    anchor = (pos, Instant::now());
                }
                let t = if s.state == PlaybackState::Playing {
                    pos + anchor.1.elapsed().as_millis() as u64
                } else {
                    pos
                };
                let phase = if bpm > 0.0 {
                    (t as f64 * f64::from(bpm) / 60_000.0).fract() as f32
                } else {
                    0.0
                };
                let mut buf = vec![0u8; sirtone_audio::viz::FRAME];
                source.frame(&mut buf, t, phase, bpm);
                if channel.send(VizFrame(buf)).is_err() {
                    break;
                }
                next += PERIOD;
                let now = Instant::now();
                if next > now {
                    std::thread::sleep(next - now);
                } else {
                    // Behind (a busy machine): no catching up, the newest frame next.
                    next = now;
                }
            }
        });
}

/// The colours for the «Волны» palette of a track (T4.2): its vibe, its cover's palette and the
/// collection colour. Covers are decoded off the IPC thread.
#[tauri::command]
#[specta::specta]
pub async fn viz_colors(
    app: AppHandle,
    track_id: Option<u32>,
    cover: Option<String>,
) -> Result<VizColors, sirtone_core::error::AppError> {
    tauri::async_runtime::spawn_blocking(move || {
        let lib = app.state::<Library>();
        let v: [Option<f64>; 6] = track_id
            .and_then(|t| {
                lib.with_read(|c| {
                    Ok(c.query_row(
                        "SELECT valence, arousal, energy, aggression, danceability, acousticness
                           FROM track WHERE id = ?1",
                        [t],
                        |r| {
                            let mut v = [None; 6];
                            for (i, x) in v.iter_mut().enumerate() {
                                *x = r.get::<_, Option<f64>>(i)?;
                            }
                            Ok(v)
                        },
                    )?)
                })
                .ok()
            })
            .unwrap_or([None; 6]);
        let [
            valence,
            arousal,
            energy,
            aggression,
            danceability,
            acousticness,
        ] = v;
        let Some(root) = lib.root() else {
            return VizColors {
                valence,
                arousal,
                energy,
                aggression,
                danceability,
                acousticness,
                cover: Vec::new(),
                collection: Vec::new(),
            };
        };
        let cover = cover
            .map(|h| palette_of(&app, &root, &h))
            .unwrap_or_default()
            .into_iter()
            .map(VizSwatch::from)
            .collect();
        let viz = app.state::<Viz>();
        let fresh = viz
            .collection
            .lock()
            .ok()
            .and_then(|c| c.clone())
            .filter(|(at, _)| at.elapsed() < COLLECTION_TTL);
        let collection = match fresh {
            Some((_, c)) => c,
            None => {
                let albums = lib
                    .with_read(palette::collection_covers)
                    .unwrap_or_default();
                let palettes: Vec<(Vec<Swatch>, f32)> = albums
                    .iter()
                    .map(|(h, w)| (palette_of(&app, &root, h), *w))
                    .collect();
                let c: Vec<VizSwatch> = palette::collection(&palettes)
                    .into_iter()
                    .map(VizSwatch::from)
                    .collect();
                if let Ok(mut slot) = viz.collection.lock() {
                    *slot = Some((Instant::now(), c.clone()));
                }
                c
            }
        };
        VizColors {
            valence,
            arousal,
            energy,
            aggression,
            danceability,
            acousticness,
            cover,
            collection,
        }
    })
    .await
    .map_err(|e| {
        sirtone_core::error::AppError::new(sirtone_core::error::ErrorCode::Internal).with_details(e)
    })
}

/// Stops the feed (the visualiser left the screen or the window is hidden).
#[tauri::command]
#[specta::specta]
pub fn viz_stop(viz: State<'_, Viz>) {
    viz.stop();
}
