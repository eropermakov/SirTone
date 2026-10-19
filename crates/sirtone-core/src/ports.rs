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
    /// TOTALDISCS / TPOS «n/m» — decides `1-01 - …` file names (docs/05 §2.1).
    pub disc_total: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    /// MusicBrainz recording id (Picard writes it): popularity for «Для всех» (T3.32).
    pub mbid_recording: Option<String>,
    /// MusicBrainz release and release group ids: covers from the Cover Art Archive (T3.3).
    pub mbid_release: Option<String>,
    pub mbid_group: Option<String>,
}

/// A MusicBrainz id: 36 characters, hex digits and dashes.
pub fn valid_mbid(s: &str) -> bool {
    s.len() == 36 && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
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
    pub disc_total: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    /// 0…1.
    pub confidence: f64,
    /// `None` = confident enough for the canonical place; `Some` = `_Unknown` with this reason.
    pub unknown: Option<UnknownReason>,
}

/// What the source location says about a file (docs/05 §4.1): parent folders and the batch-wide
/// consensus of tags in that folder (≥ 3 files, mode of album/album artist).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FolderHints {
    /// Nearest folder name (`Альбом (2004)`).
    pub parent: Option<String>,
    /// The folder above it (`Би-2`).
    pub grandparent: Option<String>,
    pub consensus_album: Option<String>,
    pub consensus_album_artist: Option<String>,
}

pub trait Identify: Send + Sync {
    /// `file_name` is the source file name with extension.
    fn identify(
        &self,
        tags: &Tags,
        file_name: &str,
        folders: &FolderHints,
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
    /// Stable file identity for move detection (docs/05 §7): `<volume serial>:<file id>` on
    /// NTFS/ReFS only — FAT/exFAT reuse indexes, so there it is `None` and the hash decides.
    fn file_id(&self, _path: &Path) -> Option<String> {
        None
    }
}

/// Facts about the volume a path is on.
pub trait VolumeInfo: Send + Sync {
    fn is_removable(&self, path: &Path) -> bool;
    fn free_bytes(&self, path: &Path) -> Option<u64>;
}

/// Where a cover came from (docs/03 §7 `cover.source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverSource {
    Embedded,
    Folder,
    /// Cover Art Archive (FR-REC-009, online).
    Caa,
}

impl CoverSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "embedded",
            Self::Folder => "folder",
            Self::Caa => "caa",
        }
    }
}

/// Cover art for a file (FR-REC-009 offline part): embedded picture, else an image next to the
/// source file (`cover`/`folder`/`front`). Online sources (Cover Art Archive) come with Stage 3.
pub trait CoverReader: Send + Sync {
    /// `file` is read for an embedded picture, `source_dir` is searched for a folder image.
    /// Returns the raw image bytes (JPEG/PNG) and their source.
    fn read(
        &self,
        file: &std::path::Path,
        source_dir: Option<&std::path::Path>,
    ) -> Option<(Vec<u8>, CoverSource)>;
}

/// Streaming PCM for analysis (T3.6): decodes a file packet by packet and hands interleaved
/// `f32` frames to `sink(frames, channels, rate)`; `false` from the sink stops early. Never
/// the whole track in memory. Implemented by `sirtone-audio`.
pub trait PcmDecoder: Send + Sync {
    fn decode(
        &self,
        path: &std::path::Path,
        sink: &mut dyn FnMut(&[f32], usize, u32) -> bool,
    ) -> Result<(), crate::error::AppError>;
}

/// Cached HTTP responses (T3.1, `03` §11): `sirtone-net` reads and writes, the library keeps
/// them (`http_cache`). Keys are full URLs; stale entries are not returned.
pub trait HttpCache: Send + Sync {
    fn get(&self, key: &str, now_ms: i64) -> Option<Vec<u8>>;
    fn put(&self, key: &str, body: &[u8], now_ms: i64, ttl_s: i64);
}
