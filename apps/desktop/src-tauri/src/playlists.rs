//! Manual and smart playlists over IPC (T2.11, T3.13; FR-PLS-001/002/004). Reads go through the library's read
//! connection, edits through its writer; every edit emits `PlaylistsChanged` (the sidebar and an
//! open playlist page refresh). Names come from the UI — Rust produces no user-visible text.

use serde::{Deserialize, Serialize};
use sirtone_core::error::AppError;
use sirtone_library::playlists;
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::library::{Library, TrackItem, track_item};

/// A sidebar row.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistItem {
    pub id: u32,
    pub name: String,
    pub tracks: u32,
    pub duration_ms: f64,
    pub bytes: f64,
    /// Four hashes → a 2×2 mosaic, one → a single cover, none → placeholder.
    pub covers: Vec<String>,
    /// A smart playlist: rules, read-only (no add / remove / move).
    pub smart: bool,
    /// ∞: continues with similar tracks from the library (FR-PLS-003/005).
    pub hybrid: bool,
}

/// A playlist page row: the position addresses remove / move (a track may repeat).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistEntry {
    pub position: u32,
    pub track: TrackItem,
}

/// Playlists were created, renamed, deleted or edited.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistsChanged {
    /// The edited playlist (0 when several or a deletion).
    pub id: u32,
}

pub(crate) fn changed(app: &AppHandle, id: u32) {
    let _ = PlaylistsChanged { id }.emit(app);
}

fn ids(tracks: &[u32]) -> Vec<i64> {
    tracks.iter().map(|&t| i64::from(t)).collect()
}

#[tauri::command(async)]
#[specta::specta]
pub fn playlists_list(lib: State<'_, Library>) -> Result<Vec<PlaylistItem>, AppError> {
    lib.with_read(|c| {
        Ok(playlists::list(c)?
            .into_iter()
            .map(|p| PlaylistItem {
                id: p.id as u32,
                name: p.name,
                tracks: p.tracks,
                duration_ms: p.duration_ms as f64,
                bytes: p.bytes as f64,
                covers: p.covers,
                smart: p.smart,
                hybrid: p.hybrid,
            })
            .collect())
    })
}

/// A new smart playlist from rules (`05` §8.1 JSON; validated, stored canonical).
#[tauri::command(async)]
#[specta::specta]
pub fn smart_create(
    app: AppHandle,
    lib: State<'_, Library>,
    name: String,
    rules: String,
) -> Result<u32, AppError> {
    let id = lib.with_write(|c| playlists::create_smart(c, &name, &rules))? as u32;
    changed(&app, id);
    Ok(id)
}

/// New rules for a smart playlist.
#[tauri::command(async)]
#[specta::specta]
pub fn smart_set_rules(
    app: AppHandle,
    lib: State<'_, Library>,
    id: u32,
    rules: String,
) -> Result<(), AppError> {
    lib.with_write(|c| playlists::set_rules(c, i64::from(id), &rules))?;
    changed(&app, id);
    Ok(())
}

/// The rules of a smart playlist (canonical JSON), `None` for a manual one.
#[tauri::command(async)]
#[specta::specta]
pub fn smart_rules(lib: State<'_, Library>, id: u32) -> Result<Option<String>, AppError> {
    lib.with_read(|c| Ok(playlists::rules(c, i64::from(id))?.map(|r| r.to_json())))
}

/// «Подходит N треков» for rules being edited (before the limit); bad rules → InvalidArgument.
#[tauri::command(async)]
#[specta::specta]
pub fn smart_preview(lib: State<'_, Library>, rules: String) -> Result<u32, AppError> {
    let r = sirtone_library::smart::Rules::parse(&rules)?;
    let now = sirtone_library::db::now_ms();
    lib.with_read(|c| sirtone_library::smart::count(c, &r, now))
}

/// «Обновить»: a smart playlist with a delay or manual refresh is evaluated again.
#[tauri::command(async)]
#[specta::specta]
pub fn smart_refresh(app: AppHandle, lib: State<'_, Library>, id: u32) -> Result<(), AppError> {
    let now = sirtone_library::db::now_ms();
    lib.with_write(|c| playlists::refresh(c, i64::from(id), now))?;
    changed(&app, id);
    Ok(())
}

/// «Сохранить как обычный»: a manual playlist with the smart playlist's current tracks.
#[tauri::command(async)]
#[specta::specta]
pub fn smart_save_as_manual(
    app: AppHandle,
    lib: State<'_, Library>,
    id: u32,
    name: String,
) -> Result<u32, AppError> {
    let new = lib.with_write(|c| playlists::save_as_manual(c, i64::from(id), &name))? as u32;
    changed(&app, new);
    Ok(new)
}

#[tauri::command(async)]
#[specta::specta]
pub fn playlist_create(
    app: AppHandle,
    lib: State<'_, Library>,
    name: String,
) -> Result<u32, AppError> {
    let id = lib.with_write(|c| playlists::create(c, &name))? as u32;
    changed(&app, id);
    Ok(id)
}

#[tauri::command(async)]
#[specta::specta]
pub fn playlist_rename(
    app: AppHandle,
    lib: State<'_, Library>,
    id: u32,
    name: String,
) -> Result<(), AppError> {
    lib.with_write(|c| playlists::rename(c, i64::from(id), &name))?;
    changed(&app, id);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn playlist_delete(app: AppHandle, lib: State<'_, Library>, id: u32) -> Result<(), AppError> {
    lib.with_write(|c| playlists::delete(c, i64::from(id)))?;
    changed(&app, 0);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn playlist_tracks(lib: State<'_, Library>, id: u32) -> Result<Vec<PlaylistEntry>, AppError> {
    // A smart playlist with a delay: its snapshot is refreshed when stale (Navidrome
    // `refreshDelay`), live ones are evaluated by the read below.
    let now = sirtone_library::db::now_ms();
    if lib.with_read(|c| playlists::refresh_due(c, i64::from(id), now))? {
        lib.with_write(|c| playlists::refresh(c, i64::from(id), now))?;
    }
    lib.with_read(|c| {
        Ok(playlists::tracks(c, i64::from(id))?
            .into_iter()
            .map(|t| PlaylistEntry {
                position: t.position as u32,
                track: track_item(t.track),
            })
            .collect())
    })
}

/// Which of `tracks` the playlist already has («Уже в плейлисте», `01b` F-13).
#[tauri::command(async)]
#[specta::specta]
pub fn playlist_already_in(
    lib: State<'_, Library>,
    id: u32,
    tracks: Vec<u32>,
) -> Result<Vec<u32>, AppError> {
    lib.with_read(|c| {
        Ok(playlists::already_in(c, i64::from(id), &ids(&tracks))?
            .into_iter()
            .map(|t| t as u32)
            .collect())
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn playlist_add(
    app: AppHandle,
    lib: State<'_, Library>,
    id: u32,
    tracks: Vec<u32>,
) -> Result<u32, AppError> {
    let n = lib.with_write(|c| playlists::add(c, i64::from(id), &ids(&tracks)))?;
    // «+» on a recommendation: the playing playlist grows (T3.14).
    if let Some(c) = app.try_state::<crate::continuation::Continuation>() {
        c.playlist_changed(&app, i64::from(id), &tracks);
    }
    changed(&app, id);
    Ok(n)
}

/// «Убрать из плейлиста»: the items go, the tracks stay (FR-PLS-004).
#[tauri::command(async)]
#[specta::specta]
pub fn playlist_remove(
    app: AppHandle,
    lib: State<'_, Library>,
    id: u32,
    positions: Vec<u32>,
) -> Result<(), AppError> {
    let pos: Vec<i64> = positions.iter().map(|&p| i64::from(p)).collect();
    lib.with_write(|c| playlists::remove(c, i64::from(id), &pos))?;
    changed(&app, id);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn playlist_move(
    app: AppHandle,
    lib: State<'_, Library>,
    id: u32,
    from: u32,
    to: u32,
) -> Result<(), AppError> {
    lib.with_write(|c| playlists::move_item(c, i64::from(id), i64::from(from), i64::from(to)))?;
    changed(&app, id);
    Ok(())
}
