//! SirTone desktop entry point (T0.3 scaffold).
// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diagnostics;
mod settings_store;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{Manager, Theme};

/// DESIGN.md §3 `bg-base` for the current OS theme — the native window paints this before the page.
fn bg_base(theme: Theme) -> Color {
    match theme {
        Theme::Light => Color(0xf2, 0xee, 0xe6, 0xff),
        _ => Color(0x16, 0x11, 0x11, 0xff),
    }
}

/// Fallback: if the page never reports "finished" (JS error, slow disk), show the window anyway.
const SHOW_FALLBACK: Duration = Duration::from_secs(3);

/// Set once the main window has been shown, so the fallback never re-shows a window the user
/// already hid (close-to-tray, T0.10).
#[derive(Default)]
struct ShownOnce(Arc<AtomicBool>);

fn main() {
    let data_dir = diagnostics::data_dir();
    let debug = cfg!(debug_assertions) || std::env::args().any(|a| a == "--debug");
    let _log_guard = diagnostics::init_logging(&data_dir, debug);
    diagnostics::install_panic_hook(data_dir.clone());
    tracing::info!(target: "sirtone::startup", version = env!("CARGO_PKG_VERSION"), "starting");
    let settings = settings_store::SettingsStore::open(&data_dir);

    let app = tauri::Builder::default()
        // Second launch (or Jump List command) focuses the existing window (FR-SHELL-004).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(ShownOnce::default())
        .manage(settings)
        .invoke_handler(tauri::generate_handler![
            settings_store::settings_get,
            settings_store::settings_patch
        ])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let theme = window.theme().unwrap_or(Theme::Dark);
                let _ = window.set_background_color(Some(bg_base(theme)));
                let fallback = window.clone();
                let shown = app.state::<ShownOnce>().0.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(SHOW_FALLBACK);
                    if !shown.swap(true, Ordering::SeqCst) {
                        let _ = fallback.show();
                    }
                });
            }
            Ok(())
        })
        // Window starts hidden (tauri.conf.json) and is shown after the first page load: no white flash.
        .on_page_load(|webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let shown = &webview.state::<ShownOnce>().0;
                if !shown.swap(true, Ordering::SeqCst) {
                    let _ = webview.window().show();
                }
            }
        })
        .build(tauri::generate_context!());

    match app {
        Ok(app) => app.run(|_, _| {}),
        Err(err) => {
            tracing::error!(target: "sirtone::startup", error = %err, "failed to start");
            eprintln!("failed to start SirTone: {err}");
            std::process::exit(1);
        }
    }
}
