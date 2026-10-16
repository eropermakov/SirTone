//! `app_bootstrap` (T0.11, docs/03 §5): everything the window needs to draw live state in one
//! call — on first start and whenever the WebView is recreated (D-52).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::ipc::Json;
use crate::library::{Library, LibraryStatus};
use crate::playback::{Playback, PlaybackSnapshot};
use crate::settings_store::SettingsStore;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub version: String,
    pub settings: Json,
    pub playback: Option<PlaybackSnapshot>,
    pub library: Option<LibraryStatus>,
}

#[tauri::command]
#[specta::specta]
pub fn app_bootstrap(app: AppHandle, settings: State<'_, Arc<SettingsStore>>) -> Bootstrap {
    // The engine may have failed to start (no audio stack): the window still opens.
    let playback = app.try_state::<Playback>();
    Bootstrap {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        settings: Json(settings.get()),
        playback: playback.map(|p| p.snapshot()),
        library: app.try_state::<Library>().map(|l| l.status()),
    }
}
