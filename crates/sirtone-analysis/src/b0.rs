//! Analysis B0 (T3.6, docs/04 §10): the fast pass over the whole file at its native rate —
//! EBU R128 integrated loudness, true peak and LRA (`ebur128`), silence at the start and end
//! (−60 dBFS, like Mixxx Skip Silence), a 400-point peak overview for the timeline, and the
//! 10 Hz envelopes of the first/last 30 s (crossfades, soft intro/outro of the sleep wave) —
//! and, in the same decode, the fingerprint of the first 120 s (T3.2, `fingerprint.rs`).

use std::path::Path;

use ebur128::{EbuR128, Mode};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::ports::PcmDecoder;

/// Bump when the B0 algorithm changes: tracks with an older version are analysed again.
/// 2 — fingerprints (T3.2); 3 — tempo and key (B1, T3.7).
pub const VERSION: i64 = 3;

const SILENCE_DB: f32 = -60.0;
const OVERVIEW: usize = 400;
/// 30 s of 10 Hz envelope.
const ENV_WINDOWS: usize = 300;

pub use sirtone_core::analysis::B0;

fn db(v: f32) -> f32 {
    if v <= 1e-10 { -100.0 } else { 20.0 * v.log10() }
}

fn env_u8(db: f32) -> u8 {
    (db.clamp(-100.0, 0.0) + 100.0).round() as u8
}

/// Running state over the decoded frames.
#[derive(Default)]
struct Acc {
    meter: Option<EbuR128>,
    channels: usize,
    rate: u32,
    win: usize,
    /// Per 100 ms window: mean square (all channels) and peak.
    ms: Vec<f32>,
    pk: Vec<f32>,
    sum: f64,
    peak: f32,
    in_win: usize,
    frames: u64,
    failed: Option<String>,
}

impl Acc {
    fn feed(&mut self, frames: &[f32], channels: usize, rate: u32) -> bool {
        if self.meter.is_none() {
            match EbuR128::new(channels as u32, rate, Mode::I | Mode::LRA | Mode::TRUE_PEAK) {
                Ok(m) => self.meter = Some(m),
                Err(e) => {
                    self.failed = Some(e.to_string());
                    return false;
                }
            }
            self.channels = channels.max(1);
            self.rate = rate;
            self.win = (rate as usize / 10).max(1);
        }
        if let Some(m) = self.meter.as_mut()
            && let Err(e) = m.add_frames_f32(frames)
        {
            self.failed = Some(e.to_string());
            return false;
        }
        for frame in frames.chunks(self.channels) {
            for &v in frame {
                self.sum += f64::from(v * v);
                self.peak = self.peak.max(v.abs());
            }
            self.in_win += 1;
            self.frames += 1;
            if self.in_win == self.win {
                self.close_window();
            }
        }
        true
    }

    fn close_window(&mut self) {
        self.ms
            .push((self.sum / (self.in_win * self.channels) as f64) as f32);
        self.pk.push(self.peak);
        self.sum = 0.0;
        self.peak = 0.0;
        self.in_win = 0;
    }
}

pub fn analyze(dec: &dyn PcmDecoder, path: &Path) -> Result<B0, AppError> {
    let mut a = Acc::default();
    let mut fp = crate::fingerprint::FpAcc::default();
    let mut b1 = crate::b1::B1Acc::default();
    dec.decode(path, &mut |frames, channels, rate| {
        fp.feed(frames, channels, rate);
        b1.feed(frames, channels, rate);
        a.feed(frames, channels, rate)
    })?;
    let fingerprint = fp.finish();
    let b1 = b1.finish();
    if let Some(e) = a.failed.take() {
        return Err(AppError::new(ErrorCode::Internal).with_details(e));
    }
    if a.in_win > 0 {
        a.close_window();
    }
    let Some(m) = a.meter.as_ref() else {
        return Err(AppError::new(ErrorCode::UnsupportedFormat).with_details("no audio"));
    };
    let finite = |v: f64| (v.is_finite() && v > -70.0).then_some(v as f32);
    let lufs_i = m.loudness_global().ok().and_then(finite);
    let lra = m.loudness_range().ok().map(|v| v as f32);
    let true_peak_db = (0..a.channels as u32)
        .filter_map(|c| m.true_peak(c).ok())
        .fold(None::<f64>, |acc, v| Some(acc.map_or(v, |x| x.max(v))))
        .map(|v| db(v as f32));
    let pk = &a.pk;
    // Silence: the first/last 100 ms window above −60 dBFS.
    let loud = |p: &f32| db(*p) > SILENCE_DB;
    let lead = pk.iter().position(loud).unwrap_or(pk.len());
    let tail = pk.iter().rev().position(loud).unwrap_or(pk.len());
    // Overview: 400 buckets of the window peaks.
    let mut peaks = vec![0u8; OVERVIEW];
    if !pk.is_empty() {
        for (i, out) in peaks.iter_mut().enumerate() {
            let lo = (i * pk.len() / OVERVIEW).min(pk.len() - 1);
            let hi = ((i + 1) * pk.len() / OVERVIEW).clamp(lo + 1, pk.len());
            let p = pk[lo..hi].iter().copied().fold(0f32, f32::max);
            *out = (p.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    let env: Vec<u8> =
        a.ms.iter()
            .map(|&v| env_u8(10.0 * v.max(1e-20).log10()))
            .collect();
    let env_head = env[..env.len().min(ENV_WINDOWS)].to_vec();
    let env_tail = env[env.len().saturating_sub(ENV_WINDOWS)..].to_vec();
    Ok(B0 {
        duration_ms: a.frames * 1000 / u64::from(a.rate.max(1)),
        lufs_i,
        true_peak_db,
        lra,
        peaks,
        lead_silence_ms: lead as u64 * 100,
        tail_silence_ms: tail as u64 * 100,
        env_head,
        env_tail,
        fp_duration_ms: fingerprint.as_ref().map_or(0, |f| f.2),
        fp_sketch: fingerprint.as_ref().map(|f| f.1.clone()),
        fingerprint: fingerprint.map(|f| f.0),
        bpm: b1.as_ref().map(|b| b.bpm),
        bpm_conf: b1.as_ref().map(|b| b.bpm_conf),
        key_camelot: b1.as_ref().map(|b| b.camelot.clone()),
        key_conf: b1.as_ref().map(|b| b.key_conf),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic decoder: a 1 kHz sine at `amp` with a 3-second fade-in, framed by silence,
    /// stereo 48 kHz.
    struct Sine {
        secs: f32,
        amp: f32,
        lead: f32,
        tail: f32,
    }

    impl PcmDecoder for Sine {
        fn decode(
            &self,
            _: &Path,
            sink: &mut dyn FnMut(&[f32], usize, u32) -> bool,
        ) -> Result<(), AppError> {
            let rate = 48_000u32;
            let total = ((self.lead + self.secs + self.tail) * rate as f32) as usize;
            let mut buf = Vec::with_capacity(8192);
            for i in 0..total {
                let t = i as f32 / rate as f32;
                let on = t >= self.lead && t < self.lead + self.secs;
                let fade = ((t - self.lead) / 3.0).clamp(0.0, 1.0);
                let v = if on {
                    self.amp * fade * (std::f32::consts::TAU * 1000.0 * t).sin()
                } else {
                    0.0
                };
                buf.push(v);
                buf.push(v);
                if buf.len() == 8192 {
                    sink(&buf, 2, rate);
                    buf.clear();
                }
            }
            if !buf.is_empty() {
                sink(&buf, 2, rate);
            }
            Ok(())
        }
    }

    #[test]
    fn loudness_silence_overview_and_soft_intro() {
        let dec = Sine {
            secs: 20.0,
            amp: 0.5,
            lead: 1.0,
            tail: 2.0,
        };
        let b = analyze(&dec, Path::new("x")).expect("analysed");
        assert_eq!(b.duration_ms, 23_000);
        // BS.1770: a full-scale 1 kHz sine is −3.01 LUFS per channel; two channels +3 dB, −6 dB
        // of amplitude → about −6 LUFS.
        let lufs = b.lufs_i.expect("measured");
        assert!((lufs - (-6.0)).abs() < 0.5, "{lufs}");
        let tp = b.true_peak_db.expect("measured");
        assert!((tp - (-6.02)).abs() < 0.3, "{tp}");
        assert_eq!((b.lead_silence_ms, b.tail_silence_ms), (1000, 2000));
        assert_eq!(b.peaks.len(), 400);
        assert_eq!(b.env_head.len(), 230);
        // After the 1 s of silence: the 3 s fade-in reads as a soft intro.
        let soft = sirtone_core::wave::soft_intro(&b.env_head[10..]).expect("long enough");
        assert!(soft > 0.3, "{soft}");
        // The fingerprint comes from the same decode, over the whole 23 s (< 120 s).
        let fp = b.fingerprint.as_deref().expect("fingerprinted");
        assert_eq!(b.fp_duration_ms, 23_000);
        assert!(crate::fingerprint::decompress(fp).is_some_and(|v| v.len() > 100));
    }
}
