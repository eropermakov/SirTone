//! The visualiser feed (T4.1, FR-VIZ-001/007, `04` §9, D-14): while a visualiser is on screen,
//! a 60 Hz thread sends frames of what is being heard over a Tauri `Channel` as raw bytes (S-04:
//! no loss, jitter p99 < 1 ms). The position and the beat phase follow the player: the snapshot
//! (4 Hz) is extrapolated between updates; the tempo comes from the track's analysis (B1).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

/// Stops the feed (the visualiser left the screen or the window is hidden).
#[tauri::command]
#[specta::specta]
pub fn viz_stop(viz: State<'_, Viz>) {
    viz.stop();
}
