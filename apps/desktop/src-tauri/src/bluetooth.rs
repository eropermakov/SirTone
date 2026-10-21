//! The Bluetooth menu in the player (T5.4, FR-WIN-006/007, `07` §6): paired audio devices,
//! «Подключить» / «Отключить» (ToothTray path, `sirtone_platform::bluetooth`), Windows'
//! Bluetooth page as the last resort. Each call runs on its own COM thread. Test seam: with
//! `SIRTONE_DATA_DIR` and `SIRTONE_SMOKE_BT_DRY` nothing is sent to a device — the request is
//! only logged, so a test never connects or disconnects the owner's headphones.

use serde::Serialize;
use sirtone_core::error::{AppError, ErrorCode};

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BtDevice {
    pub name: String,
    pub connected: bool,
    /// Its endpoint ids (the output picker may choose one).
    pub endpoints: Vec<String>,
}

fn dry() -> bool {
    std::env::var_os("SIRTONE_DATA_DIR").is_some()
        && std::env::var_os("SIRTONE_SMOKE_BT_DRY").is_some()
}

fn on_com<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, AppError> {
    std::thread::Builder::new()
        .name("sirtone-bt".into())
        .spawn(move || {
            sirtone_platform::sessions::com_init();
            f()
        })
        .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))?
        .join()
        .map_err(|_| AppError::new(ErrorCode::Internal))
}

#[tauri::command(async)]
#[specta::specta]
pub fn bt_devices() -> Result<Vec<BtDevice>, AppError> {
    on_com(|| {
        sirtone_platform::bluetooth::devices()
            .into_iter()
            .map(|d| BtDevice {
                name: d.name,
                connected: d.connected,
                endpoints: d.endpoints,
            })
            .collect()
    })
}

/// «Подключить» / «Отключить»; `false` — the device did not take it (then the UI offers the
/// Windows page).
#[tauri::command(async)]
#[specta::specta]
pub fn bt_set(name: String, connect: bool) -> Result<bool, AppError> {
    if name.is_empty() || name.len() > 256 {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    if dry() {
        tracing::info!(target: "sirtone::bt", %name, connect, "dry run — nothing sent");
        return Ok(true);
    }
    tracing::info!(target: "sirtone::bt", %name, connect, "bluetooth request");
    on_com(move || sirtone_platform::bluetooth::set_connected(&name, connect))
}

#[tauri::command]
#[specta::specta]
pub fn bt_open_settings() {
    if dry() {
        tracing::info!(target: "sirtone::bt", "dry run — settings not opened");
        return;
    }
    sirtone_platform::bluetooth::open_settings();
}
