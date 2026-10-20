//! SirTone desktop entry point (T0.3 scaffold).
// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod analysis;
mod artists;
mod autostart;
mod backgrounds;
mod beatgrid;
mod bootstrap;
mod continuation;
mod covers_online;
mod diagnostics;
mod duck;
mod duplicates;
mod feedback;
mod focus;
mod hotkeys;
mod ipc;
mod journal;
mod library;
mod net;
mod night;
mod perf;
mod playback;
mod playlists;
mod popularity;
mod presets;
mod resolve;
mod search;
mod settings_store;
mod sleep;
mod storage;
mod tray;
mod unknowns;
mod updates;
mod viz;
mod wave;
mod window_chrome;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::webview::PageLoadEvent;
use tauri::{DragDropEvent, Manager, WindowEvent};

/// Fallback: if the page never reports "finished" (JS error, slow disk), show the window anyway.
const SHOW_FALLBACK: Duration = Duration::from_secs(3);

/// Set once the main window has been shown, so the fallback never re-shows a window the user
/// already hid (close-to-tray, T0.10).
#[derive(Default)]
struct ShownOnce(Arc<AtomicBool>);

fn main() {
    // Test isolation (stage-1 plan): with SIRTONE_DATA_DIR the instance has its own settings, logs,
    // WebView2 profile and identity, and never joins or forwards to the owner's running Сиртон.
    let isolated = std::env::var_os("SIRTONE_DATA_DIR").is_some_and(|d| !d.is_empty());
    let data_dir = diagnostics::data_dir();
    if isolated {
        // SAFETY: first statement of `main`, before any other thread exists.
        unsafe { std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", data_dir.join("webview")) };
    }
    let debug = cfg!(debug_assertions) || std::env::args().any(|a| a == "--debug");
    let _log_guard = diagnostics::init_logging(&data_dir, debug);
    diagnostics::install_panic_hook(data_dir.clone());
    tracing::info!(target: "sirtone::startup", version = env!("CARGO_PKG_VERSION"), "starting");
    let settings = settings_store::SettingsStore::open(&data_dir);

    let specta = ipc::builder();
    let mut context = tauri::generate_context!();
    if isolated {
        // Test instances get their own identity: the single-instance lock and forwarding work
        // between test instances only and never reach the owner's app (D-37 id is untouched).
        context.config_mut().identifier = "studio.serkov.sirtone.test".into();
    }
    let app = tauri::Builder::default()
        // Second launch (or Jump List command, «Открыть с помощью») focuses the existing window
        // and plays the files it was given (FR-SHELL-004).
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            tray::show_main(app);
            if let Some(pb) = app.try_state::<playback::Playback>() {
                pb.open_paths(app, &playback::paths_from_args(&args));
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(autostart::plugin())
        .plugin(hotkeys::plugin())
        .plugin(updates::plugin())
        .manage(updates::Updates::default())
        .manage(continuation::Continuation::default())
        .manage(resolve::Resolver::default())
        .manage(popularity::Popularity::default())
        .manage(covers_online::CoversOnline::default())
        .manage(viz::Viz::default())
        .manage(night::Night::default())
        .manage(beatgrid::Grids::default())
        .manage(presets::Presets::open(&data_dir))
        .manage(hotkeys::Hotkeys::default())
        // «Видео» clips with HTTP ranges (T4.4); off the main thread.
        .register_asynchronous_uri_scheme_protocol("sirtone-bg", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            let range = request
                .headers()
                .get("range")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            std::thread::spawn(move || {
                responder.respond(backgrounds::protocol(&app, &path, range.as_deref()))
            });
        })
        // Covers without IPC (docs/03 §5.6); work off the main thread.
        .register_asynchronous_uri_scheme_protocol("sirtone-cover", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(library::cover_protocol(&app, &path)));
        })
        // Started by the Run entry (`--minimized`): stay in the tray, never show the window.
        .manage(ShownOnce(Arc::new(AtomicBool::new(autostart::started_minimized()))))
        .manage(tray::TrayMenu::default())
        .manage(settings)
        .invoke_handler(specta.invoke_handler())
        // Native file drop (D-07): paths stay in Rust and go to the library import (FR-IMP-001).
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
                && let Some(lib) = window.try_state::<library::Library>()
            {
                // Video clips become «Видео» backgrounds (T4.4); everything else is music.
                let (videos, music): (Vec<_>, Vec<_>) =
                    paths.iter().cloned().partition(|p| backgrounds::is_video(p));
                if let Some(pb) = window.try_state::<playback::Playback>() {
                    pb.sfx("drop"); // T4.15
                }
                if !videos.is_empty() {
                    backgrounds::add_dropped(window.app_handle(), videos);
                }
                if !music.is_empty() {
                    lib.import_paths(window.app_handle(), music);
                }
            }
        })
        .setup(move |app| {
            specta.mount_events(app);
            if let Err(e) = tray::build(app.handle()) {
                tracing::error!(target: "sirtone::startup", error = %e, "tray failed");
            }
            let store = app.state::<Arc<settings_store::SettingsStore>>().inner().clone();
            autostart::sync(app.handle(), &store);
            updates::start(app.handle());
            app.manage(library::Library::start(app.handle().clone(), store.clone()));
            app.manage(wave::Wave::start(app.handle().clone()));
            app.manage(artists::Merges::default());
            let net_state = net::start(app.handle());
            app.manage(net_state);
            analysis::start(app.handle().clone());
            duck::start(app.handle().clone());
            app.manage(sleep::Sleep::start(app.handle().clone()));
            app.manage(focus::Focus::start(
                app.handle().clone(),
                diagnostics::data_dir(),
            ));
            resolve::start(app.handle().clone());
            match playback::Playback::start(app.handle().clone(), store) {
                Ok(pb) => {
                    let args: Vec<String> = std::env::args().collect();
                    pb.open_paths(app.handle(), &playback::paths_from_args(&args));
                    app.manage(pb);
                    hotkeys::apply(app.handle());
                }
                Err(e) => {
                    tracing::error!(target: "sirtone::startup", code = ?e.code, "audio engine failed to start");
                }
            }
            if let Some(window) = app.get_webview_window("main") {
                window_chrome::apply_os_theme(&window);
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
        .build(context);

    match app {
        Ok(app) => app.run(|handle, event| {
            if let tauri::RunEvent::Exit = event {
                // Orderly exit (docs/07 §1): journal and engine stop, hotkeys are released.
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                let _ = handle.global_shortcut().unregister_all();
                if let Some(pb) = handle.try_state::<playback::Playback>() {
                    pb.shutdown();
                }
                // A downloaded update installs on a full exit (docs/07 §9.3).
                updates::install_on_exit(handle);
            }
        }),
        Err(err) => {
            tracing::error!(target: "sirtone::startup", error = %err, "failed to start");
            eprintln!("failed to start SirTone: {err}");
            std::process::exit(1);
        }
    }
}
