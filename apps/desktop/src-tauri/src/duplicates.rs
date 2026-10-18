//! Duplicates over IPC (T3.5, FR-DUP-001…004; `05` §6): the pairs found by fingerprints after
//! the background analysis, the exact copies imports skipped, and the decisions — «оставить
//! оба», or one track merged into the other (statistics, journal, ♡, tags, playlists move) with
//! the dropped file going to the Recycle Bin through the journal.

use serde::{Deserialize, Serialize};
use sirtone_core::error::AppError;
use sirtone_library::duplicates::{self, Class, Side};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::library::{Library, TrackItem, track_item};
use crate::playback::Playback;

/// New pairs were found (the analysis thread); `pending` — how many wait for a decision.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesFound {
    pub pending: u32,
}

pub fn found(app: &AppHandle, pending: u32) {
    let _ = DuplicatesFound { pending }.emit(app);
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DupSide {
    pub track: TrackItem,
    pub codec: String,
    pub lossless: bool,
    pub bitrate_kbps: Option<u32>,
    pub size_bytes: f64,
    pub plays: u32,
    /// The better file of the two (lossless > lossy > bitrate > size).
    pub better: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DupPair {
    /// same_sound · similar
    pub class: String,
    pub similarity: f32,
    /// The earlier track.
    pub a: DupSide,
    /// The later one («новый»).
    pub b: DupSide,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DuplicatesView {
    pub pairs: Vec<DupPair>,
    /// Exact copies the imports did not add (source paths, latest first).
    pub exact: Vec<String>,
}

fn side(s: Side, better: bool) -> DupSide {
    DupSide {
        track: track_item(s.track),
        codec: s.codec,
        lossless: s.lossless,
        bitrate_kbps: s.bitrate_kbps,
        size_bytes: s.size_bytes as f64,
        plays: s.plays,
        better,
    }
}

#[tauri::command(async)]
#[specta::specta]
pub fn duplicates_list(lib: State<'_, Library>) -> Result<DuplicatesView, AppError> {
    lib.with_read(|c| {
        let pairs = duplicates::pending(c)?
            .into_iter()
            .map(|p| {
                let a_better = duplicates::better(&p.a, &p.b);
                let b_better = duplicates::better(&p.b, &p.a);
                DupPair {
                    class: match p.class {
                        Class::SameSound => "same_sound".into(),
                        Class::Similar => "similar".into(),
                    },
                    similarity: p.similarity,
                    a: side(p.a, a_better),
                    b: side(p.b, b_better),
                }
            })
            .collect();
        Ok(DuplicatesView {
            pairs,
            exact: duplicates::exact_copies(c, 200)?,
        })
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn duplicates_count(lib: State<'_, Library>) -> Result<u32, AppError> {
    lib.with_read(duplicates::pending_count)
}

/// «Оставить оба».
#[tauri::command(async)]
#[specta::specta]
pub fn duplicate_keep_both(lib: State<'_, Library>, a: u32, b: u32) -> Result<(), AppError> {
    lib.with_write(|c| duplicates::keep_both(c, i64::from(a), i64::from(b)))
}

fn merge(lib: &Library, pb: &Playback, keep: u32, drop: u32) -> Result<(), AppError> {
    lib.with_write(|c| duplicates::merge(c, i64::from(keep), i64::from(drop)))?;
    pb.forget_tracks(&[drop]);
    lib.remove_track(i64::from(drop), true)
}

/// «Заменить старый новым» (`keep` = new, `drop` = old) or «Убрать новый» (the other way):
/// `keep` takes `drop`'s history, `drop`'s file goes to the Recycle Bin.
#[tauri::command(async)]
#[specta::specta]
pub fn duplicate_merge(
    lib: State<'_, Library>,
    pb: State<'_, Playback>,
    keep: u32,
    drop: u32,
) -> Result<(), AppError> {
    merge(&lib, &pb, keep, drop)
}

/// The mass action: every «тот же звук» pair keeps the better file (FR-DUP-003). Returns how
/// many files went to the Recycle Bin.
#[tauri::command(async)]
#[specta::specta]
pub fn duplicates_keep_best(
    lib: State<'_, Library>,
    pb: State<'_, Playback>,
) -> Result<u32, AppError> {
    let pairs = lib.with_read(duplicates::pending)?;
    let mut gone: Vec<i64> = Vec::new();
    for p in pairs.into_iter().filter(|p| p.class == Class::SameSound) {
        let (keep, drop) = if duplicates::better(&p.a, &p.b) {
            (p.a.track.id, p.b.track.id)
        } else {
            (p.b.track.id, p.a.track.id)
        };
        // A track already merged away in this pass (chains a↔b↔c) is skipped.
        if gone.contains(&keep) || gone.contains(&drop) {
            continue;
        }
        if let (Ok(k), Ok(d)) = (u32::try_from(keep), u32::try_from(drop)) {
            merge(&lib, &pb, k, d)?;
            gone.push(drop);
        }
    }
    Ok(gone.len() as u32)
}
