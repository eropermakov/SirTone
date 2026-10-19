//! The visualiser tap (T4.1, FR-VIZ-001/007, `04` §9, S-04/D-14): the render thread copies the
//! frames it takes from the ring — after the engine's chain (normalization; the limiter, T4.13,
//! will sit there too), before the late multipliers (volume, pause ramp) — into a lock-free side
//! ring while a visualiser is open (only copying: no allocation,
//! lock or log on the real-time thread). [`VizSource`] drains it on the visualiser's own thread
//! and builds 60 Hz frames in the S-04 format: a 32-byte header (version, flags, t_audio_ms,
//! beat_phase, bpm, bands bass/mid/treb) + `u8[1024]` mono, L, R with offset 128 (Butterchurn's
//! `timeByteArray*`). The window ends at the sample being heard now — the device buffer
//! (`padding`) is subtracted, so picture and sound agree within a frame (±30 ms, FR-VIZ-007).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

use crate::output::{DeviceFormat, Shared};

/// Samples per channel in a frame (Butterchurn's array size).
pub const WINDOW: usize = 1024;
/// Header + mono + L + R.
pub const FRAME: usize = 32 + 3 * WINDOW;
pub const VERSION: u8 = 1;
/// `flags` bit: the sound is playing (not paused or stopped).
pub const FLAG_PLAYING: u8 = 1;
/// Side ring: half a second of device frames.
pub(crate) const TAP_MS: usize = 500;
/// Frames of history kept for the window (L, R).
const HISTORY: usize = 8 * WINDOW;

/// The side ring handed from `Output::open` to the visualiser thread (a new one on every device
/// change).
pub(crate) type Tap = (rtrb::Consumer<f32>, DeviceFormat);

/// Builds visualiser frames from the tap.
pub struct VizSource {
    shared: Arc<Shared>,
    tap: Option<Tap>,
    /// Stereo history (L, R interleaved), a ring of `HISTORY` frames.
    hist: Vec<f32>,
    /// Frames received from the tap since it was (re)opened.
    received: u64,
    rate: u32,
    fft: Arc<dyn rustfft::Fft<f32>>,
    buf: Vec<Complex32>,
    hann: Vec<f32>,
}

impl VizSource {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        shared.viz_on.fetch_add(1, Ordering::AcqRel);
        let hann = (0..WINDOW)
            .map(|n| 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / (WINDOW - 1) as f32).cos())
            .collect();
        Self {
            shared,
            tap: None,
            hist: vec![0.0; HISTORY * 2],
            received: 0,
            rate: 48_000,
            fft: FftPlanner::<f32>::new().plan_fft_forward(WINDOW),
            buf: vec![Complex32::new(0.0, 0.0); WINDOW],
            hann,
        }
    }

    /// Takes everything the render thread copied since the last call.
    fn drain(&mut self) {
        if let Some(t) = self.shared.viz_tap.lock().ok().and_then(|mut s| s.take()) {
            // A new device: a new ring and maybe a new rate.
            self.rate = t.1.rate;
            self.tap = Some(t);
            self.received = 0;
            self.hist.fill(0.0);
        }
        let Some((cons, fmt)) = self.tap.as_mut() else {
            return;
        };
        let ch = fmt.channels.max(1);
        let n = cons.slots() / ch * ch;
        if n == 0 {
            return;
        }
        let Ok(chunk) = cons.read_chunk(n) else {
            return;
        };
        let (a, b) = chunk.as_slices();
        let mut frame = [0f32; 2];
        for (i, s) in a.iter().chain(b).enumerate() {
            let c = i % ch;
            if c < 2 {
                frame[c] = *s;
            }
            if c == ch - 1 {
                if ch == 1 {
                    frame[1] = frame[0];
                }
                let w = (self.received % HISTORY as u64) as usize * 2;
                self.hist[w] = frame[0];
                self.hist[w + 1] = frame[1];
                self.received += 1;
            }
        }
        chunk.commit_all();
    }

    /// The next frame into `out` (`FRAME` bytes). `t_audio_ms` — the playback position,
    /// `beat_phase` 0…1 and `bpm` from the track's analysis (0 — unknown).
    pub fn frame(&mut self, out: &mut [u8], t_audio_ms: u64, beat_phase: f32, bpm: f32) {
        debug_assert!(out.len() >= FRAME);
        self.drain();
        let playing = self.shared.playing.load(Ordering::Relaxed);
        // The window ends at the sample being heard: what the device still holds is not yet.
        let padding = self.shared.padding.load(Ordering::Relaxed);
        let end = self.received.saturating_sub(padding);
        let q = |x: f32| (128.0 + (127.0 * x.clamp(-1.0, 1.0)).round()) as u8;
        let mut energy = [0f32; 3];
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
        // Bands for the «Волны» shader: bass ≤ 250 Hz, mid ≤ 4 kHz, treble above; RMS → dB →
        // 0…1 over 60 dB.
        self.fft.process(&mut self.buf);
        let hz = self.rate as f32 / WINDOW as f32;
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

impl Drop for VizSource {
    fn drop(&mut self) {
        self.shared.viz_on.fetch_sub(1, Ordering::AcqRel);
        // The tap goes back for the next visualiser (unless a new device brought its own).
        if let (Some(t), Ok(mut slot)) = (self.tap.take(), self.shared.viz_tap.lock())
            && slot.is_none()
        {
            *slot = Some(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tone through the tap: the window holds it, aligned to what is heard; bands say bass.
    #[test]
    fn frames_follow_the_tap_minus_the_device_buffer() {
        let shared = Shared::new();
        let (mut prod, cons) = rtrb::RingBuffer::<f32>::new(48_000);
        *shared.viz_tap.lock().unwrap() = Some((
            cons,
            DeviceFormat {
                rate: 48_000,
                channels: 2,
            },
        ));
        let mut v = VizSource::new(shared.clone());
        assert_eq!(shared.viz_on.load(Ordering::Relaxed), 1);
        // 4096 frames: 100 Hz on the left, silence on the right.
        for i in 0..4096 {
            let s = (std::f32::consts::TAU * 100.0 * i as f32 / 48_000.0).sin() * 0.8;
            prod.push(s).unwrap();
            prod.push(0.0).unwrap();
        }
        shared.playing.store(true, Ordering::Relaxed);
        shared.padding.store(1024, Ordering::Relaxed);
        let mut out = vec![0u8; FRAME];
        v.frame(&mut out, 1234, 0.5, 120.0);
        assert_eq!(out[0], VERSION);
        assert_eq!(out[1], FLAG_PLAYING);
        assert_eq!(u64::from_le_bytes(out[2..10].try_into().unwrap()), 1234);
        let l = &out[32 + WINDOW..32 + 2 * WINDOW];
        let r = &out[32 + 2 * WINDOW..];
        assert!(l.iter().any(|&x| x > 200) && l.iter().any(|&x| x < 56));
        assert!(r.iter().all(|&x| x == 128));
        // The window ends 1024 frames before the last received one (the device buffer).
        let expect = |i: usize| {
            let idx = 4096 - 1024 - WINDOW + i;
            (128.0
                + (127.0 * (std::f32::consts::TAU * 100.0 * idx as f32 / 48_000.0).sin() * 0.8)
                    .round()) as u8
        };
        assert_eq!(l[0], expect(0));
        assert_eq!(l[WINDOW - 1], expect(WINDOW - 1));
        let band = |i: usize| f32::from_le_bytes(out[18 + i * 4..22 + i * 4].try_into().unwrap());
        assert!(
            band(0) > band(2) + 0.3,
            "{} {} {}",
            band(0),
            band(1),
            band(2)
        );
        drop(v);
        assert_eq!(shared.viz_on.load(Ordering::Relaxed), 0);
        // The next visualiser gets the same tap back.
        assert!(shared.viz_tap.lock().unwrap().is_some());
    }

    /// A replaced visualiser drops after its successor started: the tap stays on and the
    /// successor gets the ring (screen re-entry, StrictMode remounts).
    #[test]
    fn a_late_drop_of_the_old_source_keeps_the_new_one_fed() {
        let shared = Shared::new();
        let (mut prod, cons) = rtrb::RingBuffer::<f32>::new(48_000);
        *shared.viz_tap.lock().unwrap() = Some((
            cons,
            DeviceFormat {
                rate: 48_000,
                channels: 2,
            },
        ));
        let mut a = VizSource::new(shared.clone());
        let mut out = vec![0u8; FRAME];
        a.frame(&mut out, 0, 0.0, 0.0);
        let mut b = VizSource::new(shared.clone());
        drop(a);
        assert_eq!(shared.viz_on.load(Ordering::Relaxed), 1);
        for _ in 0..2048 {
            prod.push(0.5).unwrap();
            prod.push(0.5).unwrap();
        }
        b.frame(&mut out, 0, 0.0, 0.0);
        assert!(out[32 + WINDOW..32 + 2 * WINDOW].iter().any(|&x| x > 180));
    }
}
