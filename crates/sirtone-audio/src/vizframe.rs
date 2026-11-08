//! Visualiser frames on any platform (stage 8, D-148: the phone's visualiser). Same bytes as the
//! PC's [`crate::viz`] (S-04, `04` §9): a 32-byte header (version, flags, t_audio_ms, beat_phase,
//! bpm, bands bass/mid/treb) + `u8[1024]` mono, L, R with offset 128 — so the PC's «Волны» and
//! MilkDrop pages run unchanged. The PC builder stays where it is until stage 7 merges (it is
//! tied to the Windows output); then it can delegate here.

use std::sync::Arc;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

/// Samples per channel in a frame (Butterchurn's array size).
pub const WINDOW: usize = 1024;
/// Header + mono + L + R.
pub const FRAME: usize = 32 + 3 * WINDOW;
pub const VERSION: u8 = 1;
/// `flags` bit: the sound is playing (not paused or stopped).
pub const FLAG_PLAYING: u8 = 1;
/// Frames of history kept for the window (L, R).
const HISTORY: usize = 8 * WINDOW;

/// Keeps the last moments of what was handed to the device and builds frames from them.
pub struct FrameBuilder {
    /// Stereo history (L, R interleaved), a ring of `HISTORY` frames.
    hist: Vec<f32>,
    received: u64,
    rate: u32,
    fft: Arc<dyn rustfft::Fft<f32>>,
    buf: Vec<Complex32>,
    hann: Vec<f32>,
}

impl Default for FrameBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameBuilder {
    pub fn new() -> Self {
        let hann = (0..WINDOW)
            .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / (WINDOW - 1) as f32).cos())
            .collect();
        Self {
            hist: vec![0.0; HISTORY * 2],
            received: 0,
            rate: 48_000,
            fft: FftPlanner::<f32>::new().plan_fft_forward(WINDOW),
            buf: vec![Complex32::new(0.0, 0.0); WINDOW],
            hann,
        }
    }

    /// The sample rate of what comes next (a new track). History is kept — the picture does not
    /// blank between tracks.
    pub fn set_rate(&mut self, rate: u32) {
        self.rate = rate.max(1);
    }

    /// Interleaved samples as handed to the device; channels beyond two are dropped, mono is
    /// doubled (as the PC tap).
    pub fn push(&mut self, samples: &[f32], channels: usize) {
        let ch = channels.max(1);
        for f in samples.chunks_exact(ch) {
            let l = f[0];
            let r = if ch == 1 { l } else { f[1] };
            let w = (self.received % HISTORY as u64) as usize * 2;
            self.hist[w] = l;
            self.hist[w + 1] = r;
            self.received += 1;
        }
    }

    /// The next frame into `out` (`FRAME` bytes). `latency` — frames the device still holds
    /// (the window ends at the sample being heard, FR-VIZ-007).
    pub fn frame(
        &mut self,
        out: &mut [u8],
        playing: bool,
        t_audio_ms: u64,
        beat_phase: f32,
        bpm: f32,
        latency: u64,
    ) {
        debug_assert!(out.len() >= FRAME);
        let end = self.received.saturating_sub(latency);
        let q = |x: f32| (128.0 + (127.0 * x.clamp(-1.0, 1.0)).round()) as u8;
        for i in 0..WINDOW {
            let idx = end as i64 - WINDOW as i64 + i as i64;
            let (l, r) = if idx < 0 || (self.received - idx as u64) as usize > HISTORY {
                (0.0, 0.0)
            } else {
                let w = (idx as u64 % HISTORY as u64) as usize * 2;
                (self.hist[w], self.hist[w + 1])
            };
            let m = 0.5 * (l + r);
            out[32 + i] = q(m);
            out[32 + WINDOW + i] = q(l);
            out[32 + 2 * WINDOW + i] = q(r);
            self.buf[i] = Complex32::new(m * self.hann[i], 0.0);
        }
        // Bands for «Волны»: bass ≤ 250 Hz, mid ≤ 4 kHz, treble above; RMS → dB → 0…1 over 60 dB.
        self.fft.process(&mut self.buf);
        let hz = self.rate as f32 / WINDOW as f32;
        let mut energy = [0f32; 3];
        let mut count = [0u32; 3];
        for (k, c) in self.buf.iter().enumerate().take(WINDOW / 2).skip(1) {
            let f = k as f32 * hz;
            let b = if f < 250.0 {
                0
            } else if f < 4_000.0 {
                1
            } else {
                2
            };
            energy[b] += c.norm_sqr();
            count[b] += 1;
        }
        let bands = std::array::from_fn::<f32, 3, _>(|b| {
            let p = energy[b] / count[b].max(1) as f32 / (WINDOW as f32 * 0.25);
            let db = 10.0 * p.max(1e-12).log10();
            ((db + 60.0) / 60.0).clamp(0.0, 1.0)
        });
        out[0] = VERSION;
        out[1] = if playing { FLAG_PLAYING } else { 0 };
        out[2..10].copy_from_slice(&t_audio_ms.to_le_bytes());
        out[10..14].copy_from_slice(&beat_phase.to_le_bytes());
        out[14..18].copy_from_slice(&bpm.to_le_bytes());
        for (i, v) in bands.iter().enumerate() {
            out[18 + i * 4..22 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        out[30..32].fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The PC tap test on the shared builder: the window ends `latency` frames before the last
    /// sample, a left-only bass tone shows on the left, silence on the right, bands say bass.
    #[test]
    fn frames_match_the_pc_layout() {
        let mut b = FrameBuilder::new();
        b.set_rate(48_000);
        let mut s = Vec::new();
        for i in 0..4096 {
            s.push((std::f32::consts::TAU * 100.0 * i as f32 / 48_000.0).sin() * 0.8);
            s.push(0.0);
        }
        b.push(&s, 2);
        let mut out = vec![0u8; FRAME];
        b.frame(&mut out, true, 1234, 0.5, 120.0, 1024);
        assert_eq!(out[0], VERSION);
        assert_eq!(out[1], FLAG_PLAYING);
        assert_eq!(u64::from_le_bytes(out[2..10].try_into().unwrap()), 1234);
        let l = &out[32 + WINDOW..32 + 2 * WINDOW];
        let r = &out[32 + 2 * WINDOW..];
        assert!(r.iter().all(|&x| x == 128));
        let expect = |i: usize| {
            let idx = 4096 - 1024 - WINDOW + i;
            (128.0
                + (127.0 * (std::f32::consts::TAU * 100.0 * idx as f32 / 48_000.0).sin() * 0.8)
                    .round()) as u8
        };
        assert_eq!(l[0], expect(0));
        assert_eq!(l[WINDOW - 1], expect(WINDOW - 1));
        let band = |i: usize| f32::from_le_bytes(out[18 + i * 4..22 + i * 4].try_into().unwrap());
        assert!(band(0) > band(2) + 0.3);
    }

    /// Mono is doubled, a third channel is dropped; a new rate keeps the history.
    #[test]
    fn channels_and_track_changes() {
        let mut b = FrameBuilder::new();
        b.push(&vec![0.5; WINDOW], 1);
        b.set_rate(44_100);
        let mut out = vec![0u8; FRAME];
        b.frame(&mut out, false, 0, 0.0, 0.0, 0);
        assert_eq!(out[1], 0);
        assert!(out[32 + WINDOW..].iter().all(|&x| x == 192));
        let mut b = FrameBuilder::new();
        b.push(&[0.5, -0.5, 1.0].repeat(WINDOW), 3);
        b.frame(&mut out, true, 0, 0.0, 0.0, 0);
        assert!(out[32 + WINDOW..32 + 2 * WINDOW].iter().all(|&x| x == 192));
        assert!(out[32 + 2 * WINDOW..].iter().all(|&x| x == 64));
    }
}
