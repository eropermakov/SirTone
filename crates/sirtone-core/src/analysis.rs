//! Analysis results shared by the analyser and storage (T3.6, docs/04 §10).

/// Analysis B0: loudness, peaks, silence and envelopes of one track.
#[derive(Debug, Clone, PartialEq)]
pub struct B0 {
    pub duration_ms: u64,
    pub lufs_i: Option<f32>,
    pub true_peak_db: Option<f32>,
    pub lra: Option<f32>,
    /// 400-point overview of the peaks for the timeline.
    pub peaks: Vec<u8>,
    pub lead_silence_ms: u64,
    pub tail_silence_ms: u64,
    /// dB + 100 (−100…0 → 0…100), 10 Hz, first/last 30 s.
    pub env_head: Vec<u8>,
    pub env_tail: Vec<u8>,
    /// Chromaprint of the first 120 s, compressed (T3.2); `None` when it could not be made.
    pub fingerprint: Option<Vec<u8>>,
    pub fp_duration_ms: u64,
    /// Every 8th raw item (`fingerprint::sketch`) for the coarse duplicate check (T3.5).
    pub fp_sketch: Option<Vec<u8>>,
    /// B1 in the same pass (T3.7): tempo folded into 70–180 BPM and the key in Camelot, with
    /// the analyser's own confidences (not used as a filter — S-08).
    pub bpm: Option<f32>,
    pub bpm_conf: Option<f32>,
    pub key_camelot: Option<String>,
    pub key_conf: Option<f32>,
    /// B2/B3 (T3.8/T3.9): the vibe from the built-in tagger; `None` without it or on failure.
    pub vibe: Option<Vibe>,
    /// The tagger's embedding, `[scale f32 LE][64 × i8]` (`track.embedding`).
    pub embedding: Option<Vec<u8>>,
}

/// The vibe of a track (B3, docs/04 §10.2, docs/14 §4.1), all in [0, 1].
#[derive(Debug, Clone, PartialEq)]
pub struct Vibe {
    pub energy: f32,
    pub valence: f32,
    pub arousal: f32,
    pub danceability: f32,
    pub acousticness: f32,
    pub vocal: f32,
    pub aggression: f32,
    /// Spectral centroid, normalised (§4.1 №10).
    pub brightness: f32,
    /// весёлое, бодрое, спокойное, грустное, агрессивное, нежное, мрачное, праздничное.
    pub moods: [f32; 8],
    /// Genre group shares (sum 1): поп/танц. · рок/метал · электроника · хип-хоп/R&B ·
    /// классика/джаз/эмбиент · фолк/прочее.
    pub genres: [f32; 6],
    /// No singing to speak of (`lang = 'none'`).
    pub instrumental: bool,
    /// How complete the analysis is (two windows 1, one window less).
    pub conf: f32,
}
