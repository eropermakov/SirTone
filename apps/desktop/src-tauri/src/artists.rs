//! Artist merge over IPC (T3.12, FR-ART-003/004, AT-06): suggestions (names that differ only
//! by script or «The»), merge on the import worker (database, then each file moved by the
//! layout rules through the journal — D-70), undo within the session.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_library::artists::{self, MergeRecord};
use tauri::State;

use crate::library::Library;

/// Merges of this session, for «Отменить» (FR-ART-004).
#[derive(Default)]
pub struct Merges(Mutex<(u32, HashMap<u32, MergeRecord>)>);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRef {
    pub id: u32,
    pub name: String,
    pub tracks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtistSuggestion {
    /// More tracks — kept by default.
    pub keep: ArtistRef,
    pub drop: ArtistRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MergeDone {
    /// For `artists_merge_undo`.
    pub token: u32,
    /// Files that could not move now (locked or playing) — they stay valid where they are.
    pub failed: u32,
}

fn r(a: (i64, String, u32)) -> ArtistRef {
    ArtistRef {
        id: a.0 as u32,
        name: a.1,
        tracks: a.2,
    }
}

#[tauri::command(async)]
#[specta::specta]
pub fn artist_suggestions(lib: State<'_, Library>) -> Result<Vec<ArtistSuggestion>, AppError> {
    lib.with_read(|c| {
        Ok(artists::suggestions(c)?
            .into_iter()
            .map(|s| ArtistSuggestion {
                keep: r(s.keep),
                drop: r(s.drop),
            })
            .collect())
    })
}

/// «Не предлагать».
#[tauri::command(async)]
#[specta::specta]
pub fn artist_suggestion_dismiss(lib: State<'_, Library>, a: u32, b: u32) -> Result<(), AppError> {
    lib.with_write(|c| artists::dismiss(c, i64::from(a), i64::from(b)))
}

/// `drop` joins `keep`: its tracks, albums, ♡ / «никогда»; its name becomes an alias.
#[tauri::command(async)]
#[specta::specta]
pub fn artists_merge(
    lib: State<'_, Library>,
    merges: State<'_, Merges>,
    keep: u32,
    drop: u32,
) -> Result<MergeDone, AppError> {
    let (rec, failed) = lib.merge_artists(i64::from(keep), i64::from(drop))?;
    let mut m = merges
        .0
        .lock()
        .map_err(|_| AppError::new(ErrorCode::Internal))?;
    m.0 += 1;
    let token = m.0;
    m.1.insert(token, rec);
    Ok(MergeDone { token, failed })
}

/// «Отменить» a merge of this session.
#[tauri::command(async)]
#[specta::specta]
pub fn artists_merge_undo(
    lib: State<'_, Library>,
    merges: State<'_, Merges>,
    token: u32,
) -> Result<u32, AppError> {
    let rec = merges
        .0
        .lock()
        .map_err(|_| AppError::new(ErrorCode::Internal))?
        .1
        .remove(&token)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument).with_details("no such merge"))?;
    lib.undo_merge(rec)
}
