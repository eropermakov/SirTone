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
}
