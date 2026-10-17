//! Manual playlists over IPC (T2.11, FR-PLS-001/004). Reads go through the library's read
//! connection, edits through its writer; every edit emits `PlaylistsChanged` (the sidebar and an
//! open playlist page refresh). Names come from the UI — Rust produces no user-visible text.

use serde::{Deserialize, Serialize};
use sirtone_core::error::AppError;
use sirtone_library::playlists;
use tauri::{AppHandle, State};
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
    /// Four hashes → a 2×2 mosaic, one → a single cover, none → placeholder.
    pub covers: Vec<String>,
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

fn changed(app: &AppHandle, id: u32) {
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
                covers: p.covers,
            })
            .collect())
    })
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
