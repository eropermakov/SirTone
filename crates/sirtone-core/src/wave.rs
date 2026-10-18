//! Wave data shared between crates (docs/14 §3, §18.1): the clock, context codes and the play
//! journal. The integer codes are stored in `play_event`, `play_event_rollup` and `taste_model`
//! keys — they never change (pinned by a test).

use crate::error::AppError;

/// Every date-dependent decision (season, events, daypart, decay) takes time from here
/// (docs/14 §3.1): tests and `wave simulate` substitute it.
pub trait Clock: Send + Sync {
    fn now_utc_ms(&self) -> i64;
    /// Local UTC offset in minutes at the instant `utc_ms` (DST of that date, not of today).
    fn local_offset_min(&self, utc_ms: i64) -> i32;
}

/// Meteorological season. Stored as `u8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Season {
    Winter = 0,
    Spring = 1,
    Summer = 2,
    Autumn = 3,
}

impl Season {
    pub const ALL: [Season; 4] = [Self::Winter, Self::Spring, Self::Summer, Self::Autumn];
}

/// Time of day (FR-CTX-001). Stored as `u8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Daypart {
    Dawn = 0,
    Morning = 1,
    Day = 2,
    Evening = 3,
    Dusk = 4,
    Night = 5,
}

/// Weather class from WMO codes (docs/14 §3.3). Stored as `u8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WeatherClass {
    Clear = 0,
    Cloudy = 1,
    Fog = 2,
    DrizzleRain = 3,
    Snow = 4,
    Storm = 5,
}

/// Why a track stopped being heard (`play_event.end_reason`). Learning interprets it later;
/// no thresholds here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    Finished,
    MixedOut,
    Skip,
    Sleep,
    Stop,
    Quit,
    Error,
}

impl EndReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::MixedOut => "mixed_out",
            Self::Skip => "skip",
            Self::Sleep => "sleep",
            Self::Stop => "stop",
            Self::Quit => "quit",
            Self::Error => "error",
        }
    }
}

/// Where playback was started from (`play_event.source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaySource {
    Wave,
    Search,
    Collection,
    Playlist,
    Album,
    Artist,
    Queue,
    Hybrid,
    External,
}

impl PlaySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wave => "wave",
            Self::Search => "search",
            Self::Collection => "collection",
            Self::Playlist => "playlist",
            Self::Album => "album",
            Self::Artist => "artist",
            Self::Queue => "queue",
            Self::Hybrid => "hybrid",
            Self::External => "external",
        }
    }
}

/// The context at the moment a track starts (docs/14 §3.2, v1: clock-only fields).
#[derive(Debug, Clone, PartialEq)]
pub struct ContextSnapshot {
    pub now_utc_ms: i64,
    /// Minutes since local midnight.
    pub local_minute: u16,
    /// 0 = Monday … 6 = Sunday.
    pub weekday: u8,
    pub daypart: Daypart,
    /// Weights [winter, spring, summer, autumn], sum 1.
    pub season_w: [f32; 4],
    pub weather: Option<WeatherClass>,
}

/// One row of the internal listening journal (docs/14 §18.1 `play_event`). Never shown as
/// «History» in the UI and never leaves the machine.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayEvent {
    /// When the track became audible, unix ms UTC.
    pub ts: i64,
    pub track_id: i64,
    /// Time actually heard (pauses and seeks excluded).
    pub played_ms: u64,
    pub duration_ms: u64,
    pub end_reason: EndReason,
    pub source: PlaySource,
    pub profile: Option<String>,
    pub context: ContextSnapshot,
    pub event_id: Option<String>,
    pub novelty: Option<u8>,
    pub category: Option<u8>,
}

/// Season weights as stored: `u8[4]`, ×255 rounded.
pub fn season_w_bytes(w: &[f32; 4]) -> [u8; 4] {
    w.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Envelope sample of B0 (`env_head`/`env_tail`, 10 Hz): dB clamped to −100…0, stored +100.
pub fn env_db(v: u8) -> f32 {
    f32::from(v) - 100.0
}

/// «Мягкость вступления» (docs/14 §4.1 feature 16): the rise of the envelope over the first
/// 5 s — a fade-in is soft (1), a start at full level is hard (0). 30 dB of rise = 1.
pub fn soft_intro(env_head: &[u8]) -> Option<f32> {
    if env_head.len() < 50 {
        return None;
    }
    let mean = |w: &[u8]| w.iter().map(|&v| env_db(v)).sum::<f32>() / w.len() as f32;
    let rise = mean(&env_head[45..50]) - mean(&env_head[0..5]);
    Some((rise / 30.0).clamp(0.0, 1.0))
}

/// «Мягкость финала» (feature 17): the fall over the last 10 s — a fade-out is soft.
pub fn soft_outro(env_tail: &[u8]) -> Option<f32> {
    let n = env_tail.len();
    if n < 100 {
        return None;
    }
    let mean = |w: &[u8]| w.iter().map(|&v| env_db(v)).sum::<f32>() / w.len() as f32;
    let fall = mean(&env_tail[n - 100..n - 90]) - mean(&env_tail[n - 10..n]);
    Some((fall / 30.0).clamp(0.0, 1.0))
}

/// Number of track features (docs/14 §4.1).
pub const FEATURES: usize = 32;

/// One playable track as the wave engine sees it (docs/14 §4, §8): metadata for the feature
/// vector, statistics and the user's signals. Built by storage, consumed by `sirtone-wave`.
#[derive(Debug, Clone, PartialEq)]
pub struct WaveTrack {
    pub id: i64,
    /// Primary artist (spacing «1 из 5», «никогда» for the artist).
    pub artist_id: i64,
    pub duration_ms: u64,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub added_ms: i64,
    pub play_count: u32,
    pub skip_count: u32,
    pub last_played_ms: Option<i64>,
    pub completion: f32,
    /// The latest like/unlike is a like.
    pub liked: bool,
    /// «Никогда в волне» for the track or its artist.
    pub never: bool,
    /// Analysed features (B0–B3) with their confidence; `None` until analysed.
    pub features: Option<([f32; FEATURES], f32)>,
    /// B0 (T3.6): integrated loudness (LUFS), loudness range (LU), soft intro/outro 0…1.
    pub lufs: Option<f32>,
    pub lra: Option<f32>,
    pub soft_intro: Option<f32>,
    pub soft_outro: Option<f32>,
    /// Title, album and primary artist — season words (T3.30, docs/14 §10.2).
    pub title: Option<String>,
    pub album: Option<String>,
    pub artist_name: Option<String>,
    /// Manual season tags: bit `1 << season` for `season_*`, bit 4 for `no_season`.
    pub season_tags: u8,
    /// Weekly play ring (`plays_recent`, bucket `i` = week of the last play − i).
    pub plays_weeks: [u8; 13],
    /// B1 (T3.7): tempo (70–180 BPM) and key in Camelot («8B»).
    pub bpm: Option<f32>,
    pub key_camelot: Option<String>,
}

/// «Не сейчас» still on the shelf (docs/14 §6, D-77).
#[derive(Debug, Clone, PartialEq)]
pub struct NotNowRow {
    pub track_id: i64,
    pub ctx_key: String,
    pub expires_ms: i64,
}

/// One stored user signal (`feedback`, docs/14 §8.1): like/unlike, never/unnever, not_now.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedbackRow {
    pub ts: i64,
    pub track_id: Option<i64>,
    pub artist_id: Option<i64>,
    pub kind: String,
    pub ctx_key: Option<String>,
}

/// What the wave engine needs from storage (docs/14 §2.1). Grows with the stages that use it.
pub trait WaveRepo: Send + Sync {
    /// Appends one event in its own transaction: a crash loses at most the event in flight.
    fn record_play(&self, event: &PlayEvent) -> Result<(), AppError>;
    /// Events with `ts >= since_ms`, oldest first.
    fn plays_since(&self, since_ms: i64) -> Result<Vec<PlayEvent>, AppError>;
    /// Every playable track with what the engine scores on.
    fn catalog(&self) -> Result<Vec<WaveTrack>, AppError>;
    /// «Не сейчас» rows that have not expired at `now_ms`.
    fn not_now(&self, now_ms: i64) -> Result<Vec<NotNowRow>, AppError>;
    /// Signals with `ts >= since_ms`, oldest first.
    fn feedback_since(&self, since_ms: i64) -> Result<Vec<FeedbackRow>, AppError>;
    /// Small engine state (`wave_state`): reset and freeze marks of learning.
    fn state(&self, key: &str) -> Result<Option<String>, AppError>;
    fn set_state(&self, key: &str, value: Option<&str>) -> Result<(), AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These codes are persisted: changing one silently corrupts every stored journal.
    #[test]
    fn stored_codes_are_frozen() {
        assert_eq!(
            Season::ALL.map(|s| s as u8),
            [0, 1, 2, 3],
            "winter, spring, summer, autumn"
        );
        let dp = [
            Daypart::Dawn,
            Daypart::Morning,
            Daypart::Day,
            Daypart::Evening,
            Daypart::Dusk,
            Daypart::Night,
        ];
        assert_eq!(dp.map(|d| d as u8), [0, 1, 2, 3, 4, 5]);
        let wc = [
            WeatherClass::Clear,
            WeatherClass::Cloudy,
            WeatherClass::Fog,
            WeatherClass::DrizzleRain,
            WeatherClass::Snow,
            WeatherClass::Storm,
        ];
        assert_eq!(wc.map(|w| w as u8), [0, 1, 2, 3, 4, 5]);
        assert_eq!(EndReason::MixedOut.as_str(), "mixed_out");
        assert_eq!(PlaySource::Collection.as_str(), "collection");
        assert_eq!(season_w_bytes(&[0.76, 0.0, 0.0, 0.24]), [194, 0, 0, 61]);
    }
}
