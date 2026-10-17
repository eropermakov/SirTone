//! Updates (FR-UPD-001/002, docs/07 §9.3; pulled forward by the owner 2026-10-17, D-76):
//! 60 s after start and then once a day the installed copy asks the update channel
//! (`latest.json` on GitHub Releases) for a newer version, downloads it in the background
//! (minisign signature checked) and offers «Обновить сейчас»; otherwise it installs when the
//! user quits. Never mid-playback without consent: installing is a button or the quit itself.

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_specta::Event;

use crate::settings_store::SettingsStore;

const FIRST_CHECK: Duration = Duration::from_secs(60);
const EVERY: Duration = Duration::from_secs(24 * 3600);

/// A newer version is downloaded and ready to install.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReady {
    pub version: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current: String,
    /// Downloaded and waiting (installs at quit or on «Обновить сейчас»).
    pub ready: Option<String>,
    /// Only the installed copy updates itself (not dev builds or test instances).
    pub available: bool,
}

#[derive(Default)]
pub struct Updates {
    pending: Mutex<Option<(Update, Vec<u8>)>>,
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry, tauri_plugin_updater::Config> {
    tauri_plugin_updater::Builder::new().build()
}

/// The background schedule (installed copy only, setting `updates.auto_check`).
pub fn start(app: &AppHandle) {
    if !crate::autostart::available() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio_sleep(FIRST_CHECK).await;
        loop {
            let auto = app
                .try_state::<std::sync::Arc<SettingsStore>>()
                .is_none_or(|s| s.get()["updates"]["auto_check"].as_bool().unwrap_or(true));
            if auto && let Err(e) = check_and_download(&app).await {
                tracing::warn!(target: "sirtone::update", code = ?e.code, "update check failed");
            }
            tokio_sleep(EVERY).await;
        }
    });
}

async fn tokio_sleep(d: Duration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::NetworkOffline).with_details(e)
}

/// Asks the channel; a newer version is downloaded and kept until it is installed.
async fn check_and_download(app: &AppHandle) -> Result<Option<String>, AppError> {
    let state = app.state::<Updates>();
    if let Some((u, _)) = state.pending.lock().map_err(|_| err("lock"))?.as_ref() {
        return Ok(Some(u.version.clone()));
    }
    let shutdown_app = app.clone();
    let updater = app
        .updater_builder()
        // The installer closes us: stop playback and flush the journal first (docs/07 §9.3).
        .on_before_exit(move || {
            if let Some(pb) = shutdown_app.try_state::<crate::playback::Playback>() {
                pb.shutdown();
            }
        })
        .build()
        .map_err(err)?;
    let Some(update) = updater.check().await.map_err(err)? else {
        return Ok(None);
    };
    tracing::info!(target: "sirtone::update", version = %update.version, "update found, downloading");
    let bytes = update.download(|_, _| {}, || {}).await.map_err(err)?;
    let version = update.version.clone();
    let notes = update.body.clone();
    *state.pending.lock().map_err(|_| err("lock"))? = Some((update, bytes));
    let _ = UpdateReady {
        version: version.clone(),
        notes,
    }
    .emit(app);
    Ok(Some(version))
}

/// Installs a downloaded update (the installer closes the app and starts the new version).
fn install_pending(app: &AppHandle) -> Result<bool, AppError> {
    let Some(state) = app.try_state::<Updates>() else {
        return Ok(false);
    };
    let pending = state.pending.lock().map_err(|_| err("lock"))?.take();
    match pending {
        Some((update, bytes)) => {
            tracing::info!(target: "sirtone::update", version = %update.version, "installing update");
            update.install(bytes).map_err(err)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Called on a full exit (tray «Выход»): a waiting update installs now (docs/07 §9.3 «при выходе»).
pub fn install_on_exit(app: &AppHandle) {
    if let Err(e) = install_pending(app) {
        tracing::warn!(target: "sirtone::update", code = ?e.code, "install at exit failed");
    }
}

#[tauri::command]
#[specta::specta]
pub fn update_status(app: AppHandle, updates: State<'_, Updates>) -> UpdateStatus {
    let ready = updates
        .pending
        .lock()
        .ok()
        .and_then(|p| p.as_ref().map(|(u, _)| u.version.clone()));
    UpdateStatus {
        current: app.package_info().version.to_string(),
        ready,
        available: crate::autostart::available(),
    }
}

/// «Проверить обновления» (FR-UPD-001): returns the version found (and downloaded), if any.
#[tauri::command]
#[specta::specta]
pub async fn update_check(app: AppHandle) -> Result<Option<String>, AppError> {
    if !crate::autostart::available() {
        return Err(AppError::new(ErrorCode::InvalidArgument).with_details("not an installed copy"));
    }
    check_and_download(&app).await
}

/// «Обновить сейчас».
#[tauri::command(async)]
#[specta::specta]
pub fn update_install(app: AppHandle) -> Result<bool, AppError> {
    install_pending(&app)
}
