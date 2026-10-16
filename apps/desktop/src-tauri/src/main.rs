//! SirTone desktop entry point (T0.3 scaffold).
// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diagnostics;
mod ipc;
mod playback;
mod settings_store;
mod tray;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::webview::PageLoadEvent;
use tauri::window::Color;
use tauri::{DragDropEvent, Manager, Theme, WindowEvent};

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

    let specta = ipc::builder();
    let app = tauri::Builder::default()
        // Second launch (or Jump List command, «Открыть с помощью») focuses the existing window
        // and plays the files it was given (FR-SHELL-004).
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            tray::show_main(app);
            if let Some(pb) = app.try_state::<playback::Playback>() {
                pb.open_paths(app, &playback::paths_from_args(&args));
            }
        }))
        .manage(ShownOnce::default())
        .manage(settings)
        .invoke_handler(specta.invoke_handler())
        // Native file drop (D-07): paths stay in Rust, the engine plays the first playable one.
        .on_window_event(|window, event| {
            // Close button / Alt+F4 hide to the tray; music keeps playing (FR-SHELL-003).
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                api.prevent_close();
                let _ = window.hide();
                return;
            }
            if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event
                && let Some(pb) = window.try_state::<playback::Playback>()
            {
                pb.open_paths(window.app_handle(), paths);
            }
        })
        .setup(move |app| {
            specta.mount_events(app);
            if let Err(e) = tray::build(app.handle()) {
                tracing::error!(target: "sirtone::startup", error = %e, "tray failed");
            }
            let store = app.state::<Arc<settings_store::SettingsStore>>().inner().clone();
            match playback::Playback::start(app.handle().clone(), store) {
                Ok(pb) => {
                    let args: Vec<String> = std::env::args().collect();
                    pb.open_paths(app.handle(), &playback::paths_from_args(&args));
                    app.manage(pb);
                }
                Err(e) => {
                    tracing::error!(target: "sirtone::startup", code = ?e.code, "audio engine failed to start");
                }
            }
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
        Ok(app) => app.run(|handle, event| {
            if let tauri::RunEvent::Exit = event
                && let Some(pb) = handle.try_state::<playback::Playback>()
            {
                pb.shutdown();
            }
        }),
        Err(err) => {
            tracing::error!(target: "sirtone::startup", error = %err, "failed to start");
            eprintln!("failed to start SirTone: {err}");
            std::process::exit(1);
        }
    }
}
