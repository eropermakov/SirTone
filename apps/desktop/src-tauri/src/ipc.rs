//! IPC registry and TypeScript codegen (T0.7, D-09: tauri-specta). Hand-written IPC types in TS
//! are forbidden: `shared/ipc/bindings.ts` is generated from these commands, and a test fails
//! when the committed file differs (regenerate with `UPDATE_BINDINGS=1 cargo test -p sirtone-app`).

use tauri_specta::{Builder, collect_commands, collect_events};

use crate::{
    autostart, bootstrap, hotkeys, library, playback, playlists, search, settings_store, updates,
    window_chrome,
};

/// Free-form JSON across IPC, exported as TS `unknown`. specta inlines `serde_json::Value`
/// recursively and overflows; the UI owns the typed view of these documents.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Json(pub serde_json::Value);

impl specta::Type for Json {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        specta_typescript::Unknown::<()>::definition(types)
    }
}

pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            bootstrap::app_bootstrap,
            settings_store::settings_get,
            settings_store::settings_patch,
            playback::playback_snapshot,
            playback::playback_open,
            playback::playback_toggle,
            playback::playback_seek,
            playback::playback_set_volume,
            playback::playback_next,
            playback::playback_prev,
            playback::play_tracks,
            playback::queue_play_next,
            playback::queue_append,
            playback::queue_get,
            playback::queue_remove,
            playback::queue_move,
            playback::queue_jump,
            playback::playback_set_repeat,
            playback::playback_set_shuffle,
            autostart::autostart_available,
            hotkeys::hotkeys_status,
            updates::update_status,
            autostart::autostart_set,
            library::library_status,
            library::library_pick_folder,
            library::library_set_root,
            library::import_paths,
            library::import_cancel,
            library::library_confirm_missing,
            library::library_dismiss_question,
            library::collection_tracks,
            library::collection_albums,
            library::collection_artists,
            library::album_tracks,
            library::unknown_tracks,
            library::unknown_list,
            library::track_assign,
            library::track_details,
            library::track_remove,
            library::track_reveal,
            library::artist_removal_info,
            library::artist_remove,
            library::artist_tracks,
            library::collection_genres,
            library::genre_tracks,
            playlists::playlists_list,
            playlists::playlist_create,
            playlists::playlist_rename,
            playlists::playlist_delete,
            playlists::playlist_tracks,
            playlists::playlist_already_in,
            playlists::playlist_add,
            playlists::playlist_remove,
            playlists::playlist_move,
            search::library_search,
            window_chrome::window_set_theme,
        ])
        .events(collect_events![
            playback::PlaybackSnapshot,
            playback::PlaybackError,
            library::ImportProgress,
            library::ImportFinished,
            library::LibraryChanged,
            library::LibraryRootNeeded,
            hotkeys::HotkeyConflict,
            playback::QueueChanged,
            playlists::PlaylistsChanged
        ])
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use specta_typescript::Typescript;

    fn committed() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/shared/ipc/bindings.ts")
    }

    #[test]
    fn bindings_are_up_to_date() {
        let fresh =
            std::env::temp_dir().join(format!("sirtone-bindings-{}.ts", std::process::id()));
        super::builder()
            .export(
                Typescript::default()
                    .header("// @ts-nocheck\n/* biome-ignore-all format: generated */"),
                &fresh,
            )
            .expect("export bindings");
        let generated = std::fs::read_to_string(&fresh).expect("read generated");
        let _ = std::fs::remove_file(&fresh);
        if std::env::var_os("UPDATE_BINDINGS").is_some() {
            std::fs::write(committed(), &generated).expect("write bindings");
            return;
        }
        let current = std::fs::read_to_string(committed()).unwrap_or_default();
        assert!(
            current.replace("\r\n", "\n") == generated.replace("\r\n", "\n"),
            "IPC bindings are stale: run `UPDATE_BINDINGS=1 cargo test -p sirtone-app` and commit apps/desktop/src/shared/ipc/bindings.ts"
        );
    }
}
