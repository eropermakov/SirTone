//! Time-stretch for the automix beatmatch (T4.9c, `04` §4.4, D-100): Signalsmith Stretch (MIT,
//! Signalsmith Audio; C wrapper by Colin Marc), vendored in `vendor/signalsmith` and called
//! through this hand-written FFI. Runs on the engine thread only (never the render thread).
//!
//! Streaming start (S-08 part B): the stretcher is primed with `seek` on the first
//! `input_latency` frames, then fed from there; its first `output_latency` output frames are
//! the warm-up and are dropped. After that, output frame `k` at rate 1 is input frame `k` of the
//! fed stream.

use std::ffi::c_void;

unsafe extern "C" {
    fn signalsmith_stretch_create_preset_default(channels: i32, rate: f32) -> *mut c_void;
    fn signalsmith_stretch_destroy(h: *mut c_void);
    fn signalsmith_stretch_input_latency(h: *mut c_void) -> usize;
    fn signalsmith_stretch_output_latency(h: *mut c_void) -> usize;
    fn signalsmith_stretch_seek(h: *mut c_void, input: *mut f32, len: usize, rate: f64);
    fn signalsmith_stretch_process(
        h: *mut c_void,
        input: *mut f32,
        in_len: usize,
        out: *mut f32,
        out_len: usize,
    );
}

pub struct Stretch {
    h: *mut c_void,
    channels: usize,
}

// SAFETY: the instance is owned and used by one thread at a time (moved with its deck).
unsafe impl Send for Stretch {}

impl Stretch {
    /// `None` if the library could not create an instance.
    pub fn new(channels: usize, rate: u32) -> Option<Self> {
        // SAFETY: plain constructor; null on failure.
        let h = unsafe { signalsmith_stretch_create_preset_default(channels as i32, rate as f32) };
        (!h.is_null()).then_some(Self {
            h,
            channels: channels.max(1),
        })
    }

    pub fn input_latency(&self) -> usize {
        // SAFETY: valid handle.
        unsafe { signalsmith_stretch_input_latency(self.h) }
    }

    pub fn output_latency(&self) -> usize {
        // SAFETY: valid handle.
        unsafe { signalsmith_stretch_output_latency(self.h) }
    }

    /// Pre-roll: interleaved frames that position the stretcher without producing output.
    pub fn seek(&mut self, input: &mut [f32], rate: f64) {
        // SAFETY: the buffer holds `len / channels` whole frames.
        unsafe {
            signalsmith_stretch_seek(
                self.h,
                input.as_mut_ptr(),
                input.len() / self.channels,
                rate,
            )
        }
    }

    /// `input` frames in, `output` frames out (their ratio is the stretch).
    pub fn process(&mut self, input: &mut [f32], output: &mut [f32]) {
        // SAFETY: both buffers hold whole interleaved frames of `channels`.
        unsafe {
            signalsmith_stretch_process(
                self.h,
                input.as_mut_ptr(),
                input.len() / self.channels,
                output.as_mut_ptr(),
                output.len() / self.channels,
            )
        }
    }
}

impl Drop for Stretch {
    fn drop(&mut self) {
        // SAFETY: created by `new`, destroyed once.
        unsafe { signalsmith_stretch_destroy(self.h) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At rate 1 a primed stretcher reproduces its input after the warm-up: the alignment the
    /// automix relies on (output `k` ↔ input `k`).
    #[test]
    fn primed_at_rate_one_it_tracks_the_input() {
        let rate = 48_000;
        let n = 48_000;
        let x: Vec<f32> = (0..n)
            .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
            .collect();
        let mut s = Stretch::new(1, rate).unwrap();
        let (li, lo) = (s.input_latency(), s.output_latency());
        let mut pre = x[..li].to_vec();
        s.seek(&mut pre, 1.0);
        let mut out = Vec::new();
        let mut pos = li;
        let block = 512;
        while pos + block <= n {
            let mut i = x[pos..pos + block].to_vec();
            let mut o = vec![0f32; block];
            s.process(&mut i, &mut o);
            out.extend_from_slice(&o);
            pos += block;
        }
        let y = &out[lo..];
        // Steady part: output k ≈ input k.
        let err = (10_000..30_000)
            .map(|k| (y[k] - x[k]).abs())
            .fold(0f32, f32::max);
        assert!(err < 0.02, "max error {err}");
    }
}
