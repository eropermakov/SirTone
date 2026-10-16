//! Cross-crate traits for the import pipeline (docs/03 §3.1): `library` drives the pipeline and
//! gets format probing (`audio`), tags and identity decisions (`recognize`), and Windows file
//! operations (`platform`) through these traits; `app` wires the implementations in.

use std::path::Path;

use crate::error::AppError;

/// What the decoder layer knows about a file (FR-IMP-003, docs/05 §3 «Probed»).
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    /// `flac`, `mp3`, `m4a`, `ogg`, `opus`, `wav`, `aiff`, `wma`, … (lower case).
    pub container: String,
    /// `flac`, `mp3`, `aac`, `alac`, `vorbis`, `opus`, `pcm`, `wma`, …
    pub codec: String,
    pub lossless: bool,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub duration_ms: u64,
}

pub trait MediaProbe: Send + Sync {
    /// `Err` with `UnsupportedFormat` / `DecodeFailed` means «не добавлено» with that reason.
    fn probe(&self, path: &Path) -> Result<MediaInfo, AppError>;
}

/// Tags as read from the file (docs/05 §4.1). Empty fields are `None`/empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tags {
    pub title: Option<String>,
    /// As written; splitting into several artists is the identifier's job (docs/05 §5).
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
}

pub trait TagReader: Send + Sync {
    /// Missing or unreadable tags are not an error: they give empty `Tags`.
    fn read(&self, path: &Path) -> Tags;
}

/// Why a track lands in «Неизвестные» (docs/03 §7 `unknown_reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownReason {
    NoMetadata,
    Conflict,
    LowConfidence,
    OfflinePending,
    PossibleNonTrack,
    NeedsReview,
}

impl UnknownReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoMetadata => "no_metadata",
            Self::Conflict => "conflict",
            Self::LowConfidence => "low_confidence",
            Self::OfflinePending => "offline_pending",
            Self::PossibleNonTrack => "possible_non_track",
            Self::NeedsReview => "needs_review",
        }
    }
}

/// Offline identity decision (docs/05 §4): where the file goes and what the library records.
#[derive(Debug, Clone, PartialEq)]
pub struct IdentityDecision {
    pub title: String,
    /// Primary first, then featured.
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    /// 0…1.
    pub confidence: f64,
    /// `None` = confident enough for the canonical place; `Some` = `_Unknown` with this reason.
    pub unknown: Option<UnknownReason>,
}

pub trait Identify: Send + Sync {
    /// `file_name` is the source name, `folder` its parent folder name (folder consensus is done by
    /// the caller over a whole batch in T1.16).
    fn identify(
        &self,
        tags: &Tags,
        file_name: &str,
        folder: Option<&str>,
        info: &MediaInfo,
    ) -> IdentityDecision;
}

/// Windows file operations with the guarantees of docs/03 §8.3.
pub trait FileOps: Send + Sync {
    /// Same-volume move that is durable when it returns (`MOVEFILE_WRITE_THROUGH`); fails if
    /// `to` exists.
    fn move_durable(&self, from: &Path, to: &Path) -> std::io::Result<()>;
    /// Deletion is only ever to the Recycle Bin.
    fn to_recycle_bin(&self, path: &Path) -> std::io::Result<()>;
}

/// Facts about the volume a path is on.
pub trait VolumeInfo: Send + Sync {
    fn is_removable(&self, path: &Path) -> bool;
    fn free_bytes(&self, path: &Path) -> Option<u64>;
}
