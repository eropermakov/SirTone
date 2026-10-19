//! Analysis B1 (T3.7, `04` §10; spike S-08, D-11): tempo and key with `stratum-dsp` 1.0 in its
//! light mode — mono at ≈ 22 kHz (integer decimation), the first minute after 10 s (a shorter
//! track whole), without multi-resolution tempogram and band fusion: 97.9 % BPM ±2 and 593 ms
//! per track in S-08. The tempo is folded into 70–180 BPM (Traktor/djay range); the key is
//! given in Camelot (8B = C major, 8A = A minor). The crate's confidences are kept as they come
//! — S-08 found them uninformative, so nothing is filtered by them.

use stratum_dsp::{AnalysisConfig, Key, analyze_audio};

const TARGET_RATE: u32 = 22_050;
const SKIP_S: u32 = 10;
const WINDOW_S: u32 = 60;
/// Less than this — no tempo or key.
const MIN_S: u32 = 10;

/// Tempo and key of a track.
#[derive(Debug, Clone, PartialEq)]
pub struct B1 {
    pub bpm: f32,
    pub bpm_conf: f32,
    /// 0 = C … 11 = B.
    pub tonic: u32,
    pub major: bool,
    pub camelot: String,
    pub key_conf: f32,
}

/// Collects the analysis window from the decoded frames (the B0 pass feeds it).
#[derive(Default)]
pub struct B1Acc {
    buf: Vec<f32>,
    rate: u32,
    factor: usize,
    /// Decimation state: sum and count of the current block.
    sum: f32,
    count: usize,
    /// Output samples so far (at the decimated rate).
    seen: u64,
}

impl B1Acc {
    pub fn feed(&mut self, frames: &[f32], channels: usize, rate: u32) {
        if self.factor == 0 {
            self.factor = (rate / TARGET_RATE).max(1) as usize;
            self.rate = rate / self.factor as u32;
            self.buf.reserve(((SKIP_S + WINDOW_S) * self.rate) as usize);
        }
        let channels = channels.max(1);
        let limit = u64::from((SKIP_S + WINDOW_S) * self.rate);
        if self.seen >= limit {
            return;
        }
        for frame in frames.chunks(channels) {
            let mono = frame.iter().sum::<f32>() / channels as f32;
            self.sum += mono;
            self.count += 1;
            if self.count == self.factor {
                self.buf.push(self.sum / self.factor as f32);
                self.sum = 0.0;
                self.count = 0;
                self.seen += 1;
                if self.seen >= limit {
                    return;
                }
            }
        }
    }

    /// Tempo and key, or `None` for a track too short or a failed analysis.
    pub fn finish(self) -> Option<B1> {
        if self.rate == 0 || self.buf.len() < (MIN_S * self.rate) as usize {
            return None;
        }
        let skip = (SKIP_S * self.rate) as usize;
        // A long enough track: the minute after the intro; else all of it.
        let part = if self.buf.len() >= skip + (MIN_S * self.rate) as usize {
            &self.buf[skip..]
        } else {
            &self.buf[..]
        };
        let cfg = AnalysisConfig {
            enable_tempogram_multi_resolution: false,
            enable_tempogram_band_fusion: false,
            ..AnalysisConfig::default()
        };
        let r = std::panic::catch_unwind(|| analyze_audio(part, self.rate, cfg))
            .ok()?
            .ok()?;
        if !r.bpm.is_finite() || r.bpm <= 0.0 {
            return None;
        }
        let (tonic, major) = match r.key {
            Key::Major(i) => (i % 12, true),
            Key::Minor(i) => (i % 12, false),
        };
        Some(B1 {
            bpm: fold(r.bpm),
            bpm_conf: r.bpm_confidence.clamp(0.0, 1.0),
            tonic,
            major,
            camelot: camelot(tonic, major),
            key_conf: r.key_confidence.clamp(0.0, 1.0),
        })
    }
}

/// Into 70–180 BPM by doubling or halving.
pub fn fold(mut bpm: f32) -> f32 {
    while bpm > 0.0 && bpm < 70.0 {
        bpm *= 2.0;
    }
    while bpm > 180.0 {
        bpm /= 2.0;
    }
    bpm
}

/// Standard Camelot: 8B = C major, 8A = A minor; one step clockwise = a fifth up.
pub fn camelot(tonic: u32, major: bool) -> String {
    let rel_major = if major { tonic % 12 } else { (tonic + 3) % 12 };
    let n = ((rel_major * 7) % 12 + 7) % 12 + 1;
    format!("{n}{}", if major { "B" } else { "A" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    #[test]
    fn camelot_and_fold() {
        assert_eq!(camelot(0, true), "8B");
        assert_eq!(camelot(9, false), "8A");
        assert_eq!(camelot(7, true), "9B");
        assert_eq!(camelot(4, false), "9A");
        assert_eq!(camelot(5, true), "7B");
        assert_eq!(camelot(2, false), "7A");
        assert_eq!(fold(62.0), 124.0);
        assert_eq!(fold(186.0), 93.0);
        assert_eq!(fold(120.0), 120.0);
    }

    /// A 2-minute stereo 48 kHz loop at 124 BPM in G major: the tempo is found.
    #[test]
    fn tempo_of_a_drum_loop() {
        let rate = 48_000u32;
        let bpm = 124.0f32;
        let secs = 120.0f32;
        let n = (secs * rate as f32) as usize;
        let beat = 60.0 / bpm;
        let mut mono = vec![0f32; n];
        let mut b = 0usize;
        while (b as f32) * beat < secs {
            let i0 = (b as f32 * beat * rate as f32) as usize;
            for k in 0..(0.15 * rate as f32) as usize {
                let t = k as f32 / rate as f32;
                if let Some(o) = mono.get_mut(i0 + k) {
                    *o += 0.8 * (-t / 0.05).exp() * (TAU * 60.0 * t).sin();
                }
            }
            b += 1;
        }
        for (i, o) in mono.iter_mut().enumerate() {
            let t = i as f32 / rate as f32;
            // G major triad.
            *o += 0.05
                * ((TAU * 196.0 * t).sin() + (TAU * 246.94 * t).sin() + (TAU * 293.66 * t).sin());
        }
        let stereo: Vec<f32> = mono.iter().flat_map(|&v| [v, v]).collect();
        let mut acc = B1Acc::default();
        for chunk in stereo.chunks(8192) {
            acc.feed(chunk, 2, rate);
        }
        let r = acc.finish().expect("analysed");
        assert!((r.bpm - bpm).abs() <= 2.0, "{r:?}");
    }

    #[test]
    fn too_short_is_none() {
        let mut acc = B1Acc::default();
        acc.feed(&vec![0.1f32; 44_100 * 3], 1, 44_100);
        assert!(acc.finish().is_none());
    }
}
