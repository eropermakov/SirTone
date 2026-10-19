//! «Ночной вайб» (T4.7, FR-VIZ-009, `06` §7.5 — copy of Poweramp's Fade/Hidden controls and
//! Яндекс «Моя волна» hiding everything but the animation, `01a` §B.3/B.4): the window goes full
//! screen and the display stays awake while music plays. The request is held by one thread for
//! the whole immersive session and cleared when it ends.
//!
//! Hook for the sleep timer (T4.21, FR-SLEEP-007): in its last 10 minutes the display request
//! must be released so the screen can go dark — `release_for_sleep_timer` answers `false` until
//! the timer exists.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Manager, State};

use crate::playback::{Playback, PlaybackState};

#[derive(Default)]
pub struct Night {
    running: Mutex<Option<Arc<AtomicBool>>>,
}

/// The sleep timer's «let the screen sleep» window (T4.21).
fn release_for_sleep_timer(_app: &AppHandle) -> bool {
    false
}

impl Night {
    fn stop(&self) {
        if let Ok(mut r) = self.running.lock()
            && let Some(flag) = r.take()
        {
            flag.store(false, Ordering::Release);
        }
    }
}

/// Enters (`on`) or leaves the night vibe: full screen and the keep-awake thread.
#[tauri::command]
#[specta::specta]
pub fn night_set(app: AppHandle, night: State<'_, Night>, on: bool) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_fullscreen(on);
    }
    night.stop();
    if !on {
        return;
    }
    let flag = Arc::new(AtomicBool::new(true));
    if let Ok(mut r) = night.running.lock() {
        *r = Some(flag.clone());
    }
    let _ = std::thread::Builder::new()
        .name("sirtone-night".into())
        .spawn(move || {
            let mut held = None;
            while flag.load(Ordering::Acquire) {
                let playing = app.state::<Playback>().snapshot().state == PlaybackState::Playing;
                let want = playing && !release_for_sleep_timer(&app);
                if held != Some(want) {
                    sirtone_platform::power::keep_display_awake(want);
                    held = Some(want);
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            sirtone_platform::power::keep_display_awake(false);
        });
}
