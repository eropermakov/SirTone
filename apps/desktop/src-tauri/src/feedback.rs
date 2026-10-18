//! Player feedback over IPC (T2.19, FR-LEARN-003, FR-WENG-009): like, «не сейчас» (this
//! context bucket, then the next track), «никогда в волне» for a track or its artist, and the
//! «Никогда» list in Settings. The wave applies them from Stage 3; the facts are kept from now.

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_library::feedback;
use sirtone_platform::clock::SystemClock;
use sirtone_wave::context::ContextBuilder;
use tauri::State;

use crate::library::Library;
use crate::playback::Playback;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackFeedback {
    pub liked: bool,
    pub never_track: bool,
    pub never_artist: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum NeverScope {
    Track,
    Artist,
}

/// A «Никогда в волне» row in Settings.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct NeverEntry {
    pub scope: NeverScope,
    pub id: u32,
    pub title: String,
    /// Artists of a track; empty for an artist.
    pub subtitle: String,
}

#[tauri::command(async)]
#[specta::specta]
pub fn track_feedback(lib: State<'_, Library>, track: u32) -> Result<TrackFeedback, AppError> {
    let s = lib.with_read(|c| feedback::track_state(c, i64::from(track)))?;
    Ok(TrackFeedback {
        liked: s.liked,
        never_track: s.never_track,
        never_artist: s.never_artist,
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn feedback_like(lib: State<'_, Library>, track: u32, on: bool) -> Result<(), AppError> {
    lib.with_write(|c| feedback::set_like(c, i64::from(track), on))
}

/// «Не сейчас»: a minus in this context bucket only (D-77), and on to the next track.
#[tauri::command(async)]
#[specta::specta]
pub fn feedback_not_now(
    lib: State<'_, Library>,
    pb: State<'_, Playback>,
    track: u32,
) -> Result<(), AppError> {
    let ctx = ContextBuilder::default().snapshot(&SystemClock);
    let key = sirtone_wave::context::bucket_key(&ctx);
    lib.with_write(|c| feedback::not_now(c, i64::from(track), &key))?;
    pb.next();
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn feedback_never(
    lib: State<'_, Library>,
    scope: NeverScope,
    id: u32,
    on: bool,
) -> Result<(), AppError> {
    let id = i64::from(id);
    lib.with_write(|c| match scope {
        NeverScope::Track => feedback::set_never_track(c, id, on),
        NeverScope::Artist => feedback::set_never_artist(c, id, on),
    })
}

/// The primary artist of a track («Никогда в волне» → исполнитель).
#[tauri::command(async)]
#[specta::specta]
pub fn track_primary_artist(lib: State<'_, Library>, track: u32) -> Result<u32, AppError> {
    lib.with_read(|c| feedback::primary_artist(c, i64::from(track)))?
        .map(|a| a as u32)
        .ok_or_else(|| AppError::new(ErrorCode::Internal).with_details("no artist"))
}

#[tauri::command(async)]
#[specta::specta]
pub fn never_list(lib: State<'_, Library>) -> Result<Vec<NeverEntry>, AppError> {
    Ok(lib
        .with_read(feedback::never_list)?
        .into_iter()
        .map(|n| match n {
            feedback::NeverItem::Track { id, title, artists } => NeverEntry {
                scope: NeverScope::Track,
                id: id as u32,
                title,
                subtitle: artists,
            },
            feedback::NeverItem::Artist { id, name } => NeverEntry {
                scope: NeverScope::Artist,
                id: id as u32,
                title: name,
                subtitle: String::new(),
            },
        })
        .collect())
}

/// Manual tags of a track (FR-LEARN-013).
#[tauri::command(async)]
#[specta::specta]
pub fn track_tags(lib: State<'_, Library>, track: u32) -> Result<Vec<String>, AppError> {
    lib.with_read(|c| feedback::track_tags(c, i64::from(track)))
}

#[tauri::command(async)]
#[specta::specta]
pub fn track_tag_set(
    lib: State<'_, Library>,
    track: u32,
    tag: String,
    on: bool,
) -> Result<(), AppError> {
    lib.with_write(|c| feedback::set_tag(c, i64::from(track), &tag, on))
}
