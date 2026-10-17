//! Start with Windows (T1.24, FR-SHELL-007, docs/07 §1): HKCU\…\Run through
//! `tauri-plugin-autostart`, launched with `--minimized` (straight to the tray). Only the installed
//! copy may register itself: a dev build, a test instance or a portable copy would point Windows
//! at a path that moves or disappears.

use std::path::Path;

use sirtone_core::error::{AppError, ErrorCode};
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::settings_store::SettingsStore;

/// The flag the Run entry passes: start hidden in the tray.
pub const MINIMIZED_ARG: &str = "--minimized";

/// Name of the Run value (the owner's installed app; test instances never write it).
pub const RUN_NAME: &str = "SirTone";

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_autostart::Builder::new()
        .app_name(RUN_NAME)
        .arg(MINIMIZED_ARG)
        .build()
}

/// The NSIS per-user install (`%LOCALAPPDATA%\SirTone`, tauri.conf.json `currentUser`), not a test
/// instance.
pub fn available() -> bool {
    let isolated = std::env::var_os("SIRTONE_DATA_DIR").is_some_and(|d| !d.is_empty());
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(local) = std::env::var_os("LOCALAPPDATA") else {
        return false;
    };
    !isolated && is_installed_path(&exe, Path::new(&local))
}

fn is_installed_path(exe: &Path, local_app_data: &Path) -> bool {
    exe.parent()
        .is_some_and(|dir| dir.eq(&local_app_data.join("SirTone")))
}

/// Started by the Run entry: stay in the tray.
pub fn started_minimized() -> bool {
    std::env::args().any(|a| a == MINIMIZED_ARG)
}

/// Brings the Run entry in line with the setting at startup (an update may have moved the exe).
pub fn sync(app: &AppHandle, settings: &SettingsStore) {
    if !available() {
        return;
    }
    let want = settings.get()["window"]["autostart"]
        .as_bool()
        .unwrap_or(false);
    let al = app.autolaunch();
    let result = if want {
        al.enable()
    } else if al.is_enabled().unwrap_or(false) {
        al.disable()
    } else {
        Ok(())
    };
    if let Err(e) = result {
        tracing::warn!(target: "sirtone::shell", error = %e, "autostart sync failed");
    }
}

#[tauri::command]
#[specta::specta]
pub fn autostart_available() -> bool {
    available()
}

/// Turns «Запускать вместе с Windows» on/off and saves the setting; returns the real state.
#[tauri::command]
#[specta::specta]
pub fn autostart_set(
    app: AppHandle,
    settings: State<'_, std::sync::Arc<SettingsStore>>,
    on: bool,
) -> Result<bool, AppError> {
    if !available() {
        return Err(AppError::new(ErrorCode::InvalidArgument).with_details("not an installed copy"));
    }
    let al = app.autolaunch();
    if on { al.enable() } else { al.disable() }
        .map_err(|e| AppError::new(ErrorCode::PermissionDenied).with_details(e))?;
    settings
        .patch_durable(&serde_json::json!({ "window": { "autostart": on } }))
        .map_err(|e| AppError::new(ErrorCode::InvalidConfig).with_details(e))?;
    Ok(al.is_enabled().unwrap_or(on))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_installed_copy_qualifies() {
        let local = Path::new(r"C:\Users\u\AppData\Local");
        assert!(is_installed_path(
            Path::new(r"C:\Users\u\AppData\Local\SirTone\sirtone-app.exe"),
            local
        ));
        assert!(!is_installed_path(
            Path::new(r"D:\SirVault\SirTone\target\release\sirtone-app.exe"),
            local
        ));
        assert!(!is_installed_path(
            Path::new(r"E:\SirTone\sirtone-app.exe"),
            local
        ));
    }
}
