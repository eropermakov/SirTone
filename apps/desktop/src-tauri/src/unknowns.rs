//! «Неизвестные» II (T3.11, FR-REC-008, FR-IMP-007/008, AT-12; `01b` F-08): a hint accepted in
//! one click, the same artist for many tracks at once, «нормальный» for long files, the
//! quarantine (Recycle Bin or back) and «Повторить онлайн». Moves go through the import worker
//! and the journal; the audio files are never changed.

use sirtone_core::error::{AppError, ErrorCode};
use sirtone_library::relocate::Assignment;
use sirtone_library::unknowns;
use tauri::{AppHandle, State};

use crate::library::Library;

fn bad(what: &str) -> AppError {
    AppError::new(ErrorCode::InvalidArgument).with_details(what.to_owned())
}

/// The current title and album of a track (a mass assignment keeps them).
fn current(lib: &Library, track: i64) -> Result<(String, Option<String>), AppError> {
    lib.with_read(|c| {
        Ok(c.query_row(
            "SELECT t.title, al.title FROM track t LEFT JOIN album al ON al.id = t.album_id
             WHERE t.id = ?1",
            [track],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    })
}

/// «Принять подсказку»: the hint `index` (best first) becomes the identity; an online hint
/// confirms it (`confirmed` + its recording id).
#[tauri::command(async)]
#[specta::specta]
pub fn unknown_accept(lib: State<'_, Library>, track: u32, index: u32) -> Result<(), AppError> {
    let id = i64::from(track);
    let hints = lib.with_read(|c| unknowns::candidates(c, id))?;
    let h = hints.get(index as usize).ok_or_else(|| bad("candidate"))?;
    let (_, album) = current(&lib, id)?;
    let assignment = Assignment {
        title: h.title.clone(),
        // `assign` splits at commas: a credit with its own comma would break apart.
        artists: if h.artists.iter().any(|a| a.contains(',')) {
            h.artists[0].replace(',', " ")
        } else {
            h.artists.join(", ")
        },
        album,
        track_no: None,
        year: None,
    };
    let online = h
        .mbid
        .clone()
        .map(|m| (m, (1.0 - h.d).clamp(0.0, 1.0), None));
    lib.assign_with(id, assignment, online).map(|_| ())
}

/// One artist for many unknown tracks at once (titles and albums stay); returns how many moved.
#[tauri::command(async)]
#[specta::specta]
pub fn unknown_assign_artist(
    lib: State<'_, Library>,
    tracks: Vec<u32>,
    artist: String,
) -> Result<u32, AppError> {
    let artist = artist.trim().to_string();
    if artist.is_empty() {
        return Err(bad("artist"));
    }
    let mut moved = 0;
    for t in tracks {
        let id = i64::from(t);
        let (title, album) = current(&lib, id)?;
        let a = Assignment {
            title,
            artists: artist.clone(),
            album,
            track_no: None,
            year: None,
        };
        if lib.assign_with(id, a, None).is_ok() {
            moved += 1;
        }
    }
    Ok(moved)
}

/// «Нормальный» (FR-IMP-007): long files are music after all.
#[tauri::command(async)]
#[specta::specta]
pub fn unknown_normal(
    app: AppHandle,
    lib: State<'_, Library>,
    tracks: Vec<u32>,
) -> Result<u32, AppError> {
    let ids: Vec<i64> = tracks.iter().map(|&t| i64::from(t)).collect();
    let n = lib.with_write(|c| unknowns::mark_normal(c, &ids))?;
    lib.notify_changed(&app);
    Ok(n)
}

/// «Вернуть» from the quarantine: the track is back and analysed again.
#[tauri::command(async)]
#[specta::specta]
pub fn unknown_release(
    app: AppHandle,
    lib: State<'_, Library>,
    track: u32,
) -> Result<(), AppError> {
    lib.with_write(|c| unknowns::release(c, i64::from(track)))?;
    lib.notify_changed(&app);
    Ok(())
}

/// «Повторить онлайн»: the lookup starts over from the first unknown track now.
#[tauri::command]
#[specta::specta]
pub fn unknown_retry(app: AppHandle) -> bool {
    crate::resolve::retry(&app)
}
