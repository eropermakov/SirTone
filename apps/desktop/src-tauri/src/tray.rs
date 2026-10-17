//! Tray (T0.10, T1.21, FR-SHELL-003/006, docs/07 §1): the window's close button hides to the tray,
//! music keeps playing; «Выход» is the only full exit. Menu: «Открыть», «Играет: …», ⏯, ⏮, ⏭,
//! «Выход». Labels are read in Rust from the UI string files and every action goes straight to the
//! Rust playback service, so the tray works even when the WebView is gone (D-52).

use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::playback::Playback;

const RU_SHELL: &str = include_str!("../../../../assets/i18n/ru/shell.json");

#[derive(Debug, Clone)]
struct Labels {
    open: String,
    now_playing: String,
    nothing: String,
    play: String,
    pause: String,
    prev: String,
    next: String,
    quit: String,
    tooltip: String,
}

fn labels() -> Labels {
    let doc: serde_json::Value = serde_json::from_str(RU_SHELL).unwrap_or_default();
    let get = |k: &str, fallback: &str| {
        doc["tray"][k]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback)
            .to_owned()
    };
    Labels {
        open: get("open", "Open"),
        now_playing: get("nowPlaying", "Playing: {title}"),
        nothing: get("nothing", "Nothing is playing"),
        play: get("play", "Play"),
        pause: get("pause", "Pause"),
        prev: get("prev", "Previous"),
        next: get("next", "Next"),
        quit: get("quit", "Quit"),
        tooltip: get("tooltip", "SirTone"),
    }
}

/// Menu items that change with playback.
struct Items {
    now: MenuItem<Wry>,
    play_pause: MenuItem<Wry>,
    prev: MenuItem<Wry>,
    next: MenuItem<Wry>,
    labels: Labels,
    /// Last shown state, so unchanged snapshots do not touch the native menu.
    last: Option<TrayState>,
}

/// Native menu calls wait for the main thread (tauri `run_item_main_thread!`), so they never run
/// on the caller's thread: updates go to the `sirtone-tray` thread, latest wins.
#[derive(Default)]
pub struct TrayMenu {
    items: Mutex<Option<Items>>,
    tx: Mutex<Option<Sender<TrayState>>>,
}

/// What the tray shows about playback.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrayState {
    pub title: Option<String>,
    pub artists: Option<String>,
    pub playing: bool,
    pub has_prev: bool,
    pub has_next: bool,
}

/// Menu line and tooltip for the current track (pure: tested).
fn texts(l: &Labels, s: &TrayState) -> (String, String) {
    match &s.title {
        Some(t) => {
            let line = match &s.artists {
                Some(a) if !a.is_empty() => format!("{t} — {a}"),
                _ => t.clone(),
            };
            // Windows truncates long tray tips at 127 chars.
            let short: String = line.chars().take(60).collect();
            (
                l.now_playing.replace("{title}", &short),
                format!("{} — {short}", l.tooltip),
            )
        }
        None => (l.nothing.clone(), l.tooltip.clone()),
    }
}

/// Reflects playback in the tray (called from the engine's event sink): never blocks.
pub fn update(app: &AppHandle, s: &TrayState) {
    if let Some(menu) = app.try_state::<TrayMenu>()
        && let Some(tx) = menu.tx.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
    {
        let _ = tx.send(s.clone());
    }
}

fn apply(app: &AppHandle, s: &TrayState) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };
    let mut guard = menu.items.lock().unwrap_or_else(|p| p.into_inner());
    let Some(items) = guard.as_mut() else { return };
    if items.last.as_ref() == Some(s) {
        return;
    }
    let (line, tip) = texts(&items.labels, s);
    let _ = items.now.set_text(line);
    let _ = items.play_pause.set_text(if s.playing {
        &items.labels.pause
    } else {
        &items.labels.play
    });
    let _ = items.play_pause.set_enabled(s.title.is_some());
    let _ = items.prev.set_enabled(s.has_prev || s.title.is_some());
    let _ = items.next.set_enabled(s.has_next);
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tip));
    }
    items.last = Some(s.clone());
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn toggle_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_main(app);
        }
    }
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let pb = app.try_state::<Playback>();
    match (event.id().as_ref(), pb) {
        ("open", _) | ("now", _) => show_main(app),
        ("play_pause", Some(pb)) => pb.toggle(),
        ("prev", Some(pb)) => pb.prev(),
        ("next", Some(pb)) => pb.next(),
        ("quit", _) => app.exit(0),
        _ => {}
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<TrayIcon<Wry>> {
    let l = labels();
    let now = MenuItem::with_id(app, "now", &l.nothing, true, None::<&str>)?;
    let play_pause = MenuItem::with_id(app, "play_pause", &l.play, false, None::<&str>)?;
    let prev = MenuItem::with_id(app, "prev", &l.prev, false, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", &l.next, false, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", &l.open, true, None::<&str>)?,
            &now,
            &PredefinedMenuItem::separator(app)?,
            &play_pause,
            &prev,
            &next,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", &l.quit, true, None::<&str>)?,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(&l.tooltip)
        .on_menu_event(on_menu)
        .on_tray_icon_event(|tray, event| {
            // Left click opens/hides the window (Spotify/Discord); right click shows the menu.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    let state = app.state::<TrayMenu>();
    *state.items.lock().unwrap_or_else(|p| p.into_inner()) = Some(Items {
        now,
        play_pause,
        prev,
        next,
        labels: l,
        last: None,
    });
    let (tx, rx) = channel::<TrayState>();
    let handle = app.clone();
    std::thread::Builder::new()
        .name("sirtone-tray".into())
        .spawn(move || {
            while let Ok(mut s) = rx.recv() {
                while let Ok(newer) = rx.try_recv() {
                    s = newer;
                }
                apply(&handle, &s);
            }
        })
        .ok();
    *state.tx.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    Ok(tray)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_labels_come_from_ru_strings() {
        let l = labels();
        assert_eq!(l.quit, "Выход");
        assert_eq!(l.pause, "Пауза");
        assert!(l.now_playing.contains("{title}"));
    }

    #[test]
    fn now_playing_line_and_tooltip() {
        let l = labels();
        let s = TrayState {
            title: Some("Город".into()),
            artists: Some("Кино".into()),
            playing: true,
            ..TrayState::default()
        };
        let (line, tip) = texts(&l, &s);
        assert_eq!(line, "Играет: Город — Кино");
        assert_eq!(tip, "Сиртон — Город — Кино");
        let (line, tip) = texts(&l, &TrayState::default());
        assert_eq!(line, l.nothing);
        assert_eq!(tip, "Сиртон");
        let long = TrayState {
            title: Some("я".repeat(200)),
            ..TrayState::default()
        };
        assert!(texts(&l, &long).1.chars().count() < 127);
    }
}
