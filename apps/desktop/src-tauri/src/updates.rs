//! Seamless updates (FR-UPD-001/002; owner 2026-10-17, D-76): the user never thinks about them.
//! Like Chrome/Spotify: the installed copy asks the update channel (`latest.json` on GitHub
//! Releases) 30 s after start and then every 30 minutes, downloads a newer version in the
//! background (minisign signature checked) and installs it silently (`/S`) when nothing would be
//! interrupted — no music playing, no import running, nobody at the keyboard for 2 minutes — or
//! when the user quits. The installer starts the new version the way the old one was (window or
//! tray); the first start of a new version says «Сиртон обновлён до X».

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::settings_store::SettingsStore;

const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(30 * 60);
/// How often an idle moment for installing is looked for.
const TICK: Duration = Duration::from_secs(20);
/// Nobody touched keyboard or mouse for this long: installing interrupts no one.
const IDLE_FOR_INSTALL_MS: u64 = 2 * 60 * 1000;

/// What the UI needs: the running version and, once, «just updated».
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current: String,
    /// Set on the first start after an update (shown once as a toast).
    pub just_updated: Option<String>,
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct Updates {
    pending: Mutex<Option<(Update, Vec<u8>)>>,
    just_updated: Mutex<Option<(String, Option<String>)>>,
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::NetworkOffline).with_details(e)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Remembers which version runs now; a change since last time = «just updated».
fn note_version(app: &AppHandle) {
    let (Some(settings), Some(state)) = (
        app.try_state::<std::sync::Arc<SettingsStore>>(),
        app.try_state::<Updates>(),
    ) else {
        return;
    };
    let current = app.package_info().version.to_string();
    let doc = settings.get();
    let last = doc["updates"]["last_version"].as_str().map(str::to_owned);
    if last.as_deref() != Some(current.as_str()) {
        if last.is_some() {
            let notes = doc["updates"]["notes"].as_str().map(str::to_owned);
            tracing::info!(target: "sirtone::update", from = ?last, to = %current, "updated");
            *lock(&state.just_updated) = Some((current.clone(), notes));
        }
        let _ = settings.patch_durable(&serde_json::json!({
            "updates": { "last_version": current, "notes": null }
        }));
    }
}

/// The background loop (installed copy only, setting `updates.auto_check`).
pub fn start(app: &AppHandle) {
    note_version(app);
    if !crate::autostart::available() {
        return;
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name("sirtone-updates".into())
        .spawn(move || {
            std::thread::sleep(FIRST_CHECK);
            let mut last_check: Option<Instant> = None;
            loop {
                let auto = app
                    .try_state::<std::sync::Arc<SettingsStore>>()
                    .is_none_or(|s| s.get()["updates"]["auto_check"].as_bool().unwrap_or(true));
                if auto && last_check.is_none_or(|t| t.elapsed() >= CHECK_EVERY) {
                    last_check = Some(Instant::now());
                    if let Err(e) = tauri::async_runtime::block_on(check_and_download(&app)) {
                        tracing::info!(target: "sirtone::update", code = ?e.code, "update check skipped");
                    }
                }
                if quiet_moment(&app) {
                    install_now(&app);
                }
                std::thread::sleep(TICK);
            }
        })
        .ok();
}

/// Builds the updater: playback and the journal stop before the installer closes us.
fn updater(app: &AppHandle, minimized: bool) -> Result<tauri_plugin_updater::Updater, AppError> {
    let shutdown_app = app.clone();
    let mut b = app.updater_builder().on_before_exit(move || {
        if let Some(pb) = shutdown_app.try_state::<crate::playback::Playback>() {
            pb.shutdown();
        }
    });
    if minimized {
        // After `/ARGS` these reach the restarted app: back to the tray, as it was.
        b = b.installer_arg(crate::autostart::MINIMIZED_ARG);
    }
    b.build().map_err(err)
}

/// Asks the channel; a newer version is downloaded and kept until a quiet moment.
async fn check_and_download(app: &AppHandle) -> Result<(), AppError> {
    let state = app.state::<Updates>();
    if lock(&state.pending).is_some() {
        return Ok(());
    }
    let Some(update) = updater(app, false)?.check().await.map_err(err)? else {
        return Ok(());
    };
    tracing::info!(target: "sirtone::update", version = %update.version, "update found, downloading");
    let bytes = update.download(|_, _| {}, || {}).await.map_err(err)?;
    if let Some(settings) = app.try_state::<std::sync::Arc<SettingsStore>>() {
        let _ = settings.patch_durable(&serde_json::json!({ "updates": { "notes": update.body } }));
    }
    tracing::info!(target: "sirtone::update", version = %update.version, "update ready");
    *lock(&state.pending) = Some((update, bytes));
    Ok(())
}

/// Nothing would be interrupted: an update waits, no music, no import, nobody at the computer.
fn quiet_moment(app: &AppHandle) -> bool {
    let waiting = app
        .try_state::<Updates>()
        .is_some_and(|s| lock(&s.pending).is_some());
    if !waiting {
        return false;
    }
    let playing = app
        .try_state::<crate::playback::Playback>()
        .is_some_and(|p| p.snapshot().state == crate::playback::PlaybackState::Playing);
    let importing = app
        .try_state::<crate::library::Library>()
        .is_some_and(|l| l.status().importing);
    !playing && !importing && sirtone_platform::idle::idle_ms() >= IDLE_FOR_INSTALL_MS
}

/// Installs silently and restarts the new version in the same place (window or tray).
fn install_now(app: &AppHandle) {
    let Some(state) = app.try_state::<Updates>() else {
        return;
    };
    let Some((update, bytes)) = lock(&state.pending).take() else {
        return;
    };
    let hidden = app
        .get_webview_window("main")
        .is_none_or(|w| !w.is_visible().unwrap_or(false));
    // The relaunch arguments live in the builder: ask again with them, install the same bytes.
    let update = if hidden {
        match updater(app, true).and_then(|u| {
            tauri::async_runtime::block_on(u.check())
                .map_err(err)?
                .ok_or_else(|| err("update vanished"))
        }) {
            Ok(u) if u.version == update.version => u,
            _ => update,
        }
    } else {
        update
    };
    tracing::info!(target: "sirtone::update", version = %update.version, hidden, "installing silently");
    if let Err(e) = update.install(bytes) {
        tracing::warn!(target: "sirtone::update", error = %e, "install failed");
    }
}

/// A full exit (tray «Выход»): a waiting update installs now, without restarting the app.
pub fn install_on_exit(app: &AppHandle) {
    let Some(state) = app.try_state::<Updates>() else {
        return;
    };
    if let Some((update, bytes)) = lock(&state.pending).take() {
        tracing::info!(target: "sirtone::update", version = %update.version, "installing at exit");
        if let Err(e) = update.restart_after_install(false).install(bytes) {
            tracing::warn!(target: "sirtone::update", error = %e, "install at exit failed");
        }
    }
}

/// Version and, the first time after an update, «Сиртон обновлён до X» (taken once).
#[tauri::command]
#[specta::specta]
pub fn update_status(app: AppHandle, updates: State<'_, Updates>) -> UpdateStatus {
    let just = lock(&updates.just_updated).take();
    UpdateStatus {
        current: app.package_info().version.to_string(),
        just_updated: just.as_ref().map(|(v, _)| v.clone()),
        notes: just.and_then(|(_, n)| n),
    }
}
