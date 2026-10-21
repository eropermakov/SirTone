//! The taskbar button (T5.2, FR-WIN-004, `07` §4): ⏮ ⏯ ⏭ under the thumbnail, the track's
//! progress (≤ 2 Hz; paused — the paused state), a play/pause overlay and the Jump List tasks.
//! The COM object lives on the UI thread only (a thread-local); updates from the playback sink
//! hop there with `run_on_main_thread`. The Jump List is written once per start — not by test
//! instances (`SIRTONE_DATA_DIR`), so a test never replaces the installed app's list.

use std::cell::RefCell;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sirtone_platform::taskbar::{Progress, Taskbar, Thumb};
use tauri::{AppHandle, Manager};

use crate::playback::Playback;

thread_local! {
    static BAR: RefCell<Option<Taskbar>> = const { RefCell::new(None) };
}

/// What the button shows now (to skip repeats and to re-add after explorer restarts).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Shown {
    playing: bool,
    loaded: bool,
    position_ms: u32,
    duration_ms: u32,
}

static LAST: Mutex<Option<(Shown, Instant)>> = Mutex::new(None);
const MIN_GAP: Duration = Duration::from_millis(500);

const RU_SHELL: &str = include_str!("../../../../assets/i18n/ru/shell.json");

struct Texts {
    tips: [String; 4],
    playing: String,
    paused: String,
    tasks: [(String, String); 3],
}

fn texts() -> &'static Texts {
    static T: std::sync::OnceLock<Texts> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let doc: serde_json::Value = serde_json::from_str(RU_SHELL).unwrap_or_default();
        let get = |k: &str, d: &str| doc["tray"][k].as_str().unwrap_or(d).to_owned();
        Texts {
            tips: [
                get("prev", "Previous"),
                get("play", "Play"),
                get("pause", "Pause"),
                get("next", "Next"),
            ],
            playing: get("play", "Play"),
            paused: get("pause", "Pause"),
            tasks: [
                (get("playPause", "Play / pause"), "--cmd=toggle".into()),
                (get("next", "Next"), "--cmd=next".into()),
                (get("prev", "Previous"), "--cmd=prev".into()),
            ],
        }
    })
}

fn tips() -> [&'static str; 4] {
    let t = &texts().tips;
    [&t[0], &t[1], &t[2], &t[3]]
}

/// A Jump List command (`--cmd=toggle|next|prev`) from the arguments, if any.
pub fn command(args: &[String]) -> Option<&'static str> {
    args.iter().find_map(|a| match a.as_str() {
        "--cmd=toggle" => Some("toggle"),
        "--cmd=next" => Some("next"),
        "--cmd=prev" => Some("prev"),
        _ => None,
    })
}

pub fn run_command(app: &AppHandle, cmd: &str) {
    let Some(pb) = app.try_state::<Playback>() else {
        return;
    };
    match cmd {
        "toggle" => pb.toggle(),
        "next" => pb.next(),
        "prev" => pb.prev(),
        _ => {}
    }
}

fn add_buttons() {
    let shown = LAST
        .lock()
        .ok()
        .and_then(|l| l.map(|(s, _)| s))
        .unwrap_or_default();
    BAR.with(|b| {
        if let Some(bar) = b.borrow().as_ref() {
            bar.add(shown.playing, &tips());
            apply(bar, shown);
        }
    });
}

fn apply(bar: &Taskbar, s: Shown) {
    if !s.loaded {
        bar.progress(Progress::None, 0, 0);
        bar.overlay(None, "");
        return;
    }
    let state = if s.playing {
        Progress::Playing
    } else {
        Progress::Paused
    };
    bar.progress(
        state,
        u64::from(s.position_ms),
        u64::from(s.duration_ms.max(1)),
    );
    let t = texts();
    bar.overlay(
        Some(s.playing),
        if s.playing { &t.playing } else { &t.paused },
    );
}

/// Hooks the main window (call from `setup`).
pub fn start(app: &AppHandle) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let Ok(hwnd) = win.hwnd() else {
        return;
    };
    let hwnd = hwnd.0 as isize;
    let click_app = app.clone();
    let created_app = app.clone();
    let _ = app.run_on_main_thread(move || {
        sirtone_platform::taskbar::hook(
            hwnd,
            move |t| {
                run_command(
                    &click_app,
                    match t {
                        Thumb::Prev => "prev",
                        Thumb::Toggle => "toggle",
                        Thumb::Next => "next",
                    },
                );
            },
            move |created| {
                // The taskbar button exists (again) — for the window that got it (a rebuilt
                // window has a new handle): a fresh object, then the buttons.
                let light = !sirtone_platform::taskbar::light_taskbar();
                BAR.with(|b| *b.borrow_mut() = Taskbar::new(created, light));
                let _ = created_app.run_on_main_thread(add_buttons);
            },
        );
        // The button may already exist (the window was shown before the hook).
        let light = !sirtone_platform::taskbar::light_taskbar();
        BAR.with(|b| *b.borrow_mut() = Taskbar::new(hwnd, light));
        add_buttons();
    });
    // Jump List: once per start (not again for a rebuilt window), never by a test instance.
    static JUMP: std::sync::Once = std::sync::Once::new();
    let mut first = false;
    JUMP.call_once(|| first = true);
    if first
        && std::env::var_os("SIRTONE_DATA_DIR").is_none()
        && let Ok(exe) = std::env::current_exe()
    {
        std::thread::spawn(move || {
            sirtone_platform::sessions::com_init();
            let tasks: Vec<(String, String)> = texts().tasks.to_vec();
            if let Err(e) = sirtone_platform::taskbar::jump_list(&exe, &tasks) {
                tracing::warn!(target: "sirtone::taskbar", error = %e, "jump list failed");
            }
        });
    }
}

/// From the playback sink: the button follows (≤ 2 Hz unless play/pause changed).
pub fn update(app: &AppHandle, playing: bool, loaded: bool, position_ms: u32, duration_ms: u32) {
    let s = Shown {
        playing,
        loaded,
        position_ms,
        duration_ms,
    };
    let toggled = {
        let Ok(mut last) = LAST.lock() else { return };
        let prev = *last;
        let toggled = prev.is_none_or(|(p, _)| p.playing != playing || p.loaded != loaded);
        if !toggled && prev.is_some_and(|(_, at)| at.elapsed() < MIN_GAP) {
            return;
        }
        *last = Some((s, Instant::now()));
        toggled
    };
    let _ = app.run_on_main_thread(move || {
        BAR.with(|b| {
            if let Some(bar) = b.borrow().as_ref() {
                if toggled {
                    bar.update(s.playing, &tips());
                }
                apply(bar, s);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_list_arguments_map_to_commands() {
        let a = |s: &str| vec!["sirtone-app.exe".to_owned(), s.to_owned()];
        assert_eq!(command(&a("--cmd=toggle")), Some("toggle"));
        assert_eq!(command(&a("--cmd=next")), Some("next"));
        assert_eq!(command(&a("--cmd=prev")), Some("prev"));
        assert_eq!(command(&a("D:/music/a.flac")), None);
        assert_eq!(texts().tasks.len(), 3);
        assert!(texts().tips.iter().all(|t| !t.is_empty()));
    }
}
