//! Tray (T0.10, FR-SHELL-003/006, docs/07 §1): the window's close button hides to the tray, music
//! keeps playing; «Выход» is the only full exit. Menu labels are read in Rust from the UI string
//! files, so the tray works even when the WebView is gone (D-52).

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::playback::Playback;

const RU_SHELL: &str = include_str!("../../../../assets/i18n/ru/shell.json");

struct Labels {
    open: String,
    play_pause: String,
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
        play_pause: get("playPause", "Play / pause"),
        quit: get("quit", "Quit"),
        tooltip: get("tooltip", "SirTone"),
    }
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
    match event.id().as_ref() {
        "open" => show_main(app),
        "play_pause" => {
            if let Some(pb) = app.try_state::<Playback>() {
                pb.toggle();
            }
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<TrayIcon<Wry>> {
    let l = labels();
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", &l.open, true, None::<&str>)?,
            &MenuItem::with_id(app, "play_pause", &l.play_pause, true, None::<&str>)?,
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
    builder.build(app)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tray_labels_come_from_ru_strings() {
        let l = super::labels();
        assert_eq!(l.quit, "Выход");
        assert!(!l.open.is_empty() && !l.play_pause.is_empty());
    }
}
