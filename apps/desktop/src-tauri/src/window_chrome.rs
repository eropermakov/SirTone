//! Caption colour follows the app theme (S-20 (a), D-60). The UI resolves «как в системе» and
//! calls [`window_set_theme`] on every change; at startup Rust uses the OS theme.

use tauri::window::Color;
use tauri::{Theme, WebviewWindow};

/// DESIGN.md §3 `bg-base` — the native window paints this before the page.
pub fn bg_base(dark: bool) -> Color {
    if dark {
        Color(0x16, 0x11, 0x11, 0xff)
    } else {
        Color(0xf2, 0xee, 0xe6, 0xff)
    }
}

pub fn apply(window: &WebviewWindow, dark: bool) {
    let _ = window.set_background_color(Some(bg_base(dark)));
    if let Ok(hwnd) = window.hwnd() {
        let tinted = sirtone_platform::caption::apply(hwnd.0 as isize, dark);
        tracing::debug!(target: "sirtone::shell", dark, tinted, "caption theme");
    }
}

pub fn apply_os_theme(window: &WebviewWindow) {
    apply(
        window,
        window.theme().unwrap_or(Theme::Dark) != Theme::Light,
    );
}

/// `palette`: an owner palette (D-74) paints the caption in its own `bg-base`.
#[tauri::command]
#[specta::specta]
pub fn window_set_theme(window: WebviewWindow, dark: bool, palette: Option<String>) {
    use sirtone_platform::caption::{EMBER, MONO, apply_colors};
    let colors = match palette.as_deref() {
        Some("ember") => Some(EMBER),
        Some("mono") => Some(MONO),
        _ => None,
    };
    match colors {
        Some(c) => {
            let (r, g, b) = (
                (c.background >> 16) as u8,
                (c.background >> 8) as u8,
                c.background as u8,
            );
            let _ = window.set_background_color(Some(Color(r, g, b, 0xff)));
            if let Ok(hwnd) = window.hwnd() {
                apply_colors(hwnd.0 as isize, true, c);
            }
        }
        None => apply(&window, dark),
    }
}
