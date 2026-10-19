//! The part A corpus generator (kick on beats, snare on 2 and 4, hats on eighths, chords, bass),
//! copied from `spikes/S-08-bpmkey` to test the own beat grid on drums, not only clicks.
#![allow(dead_code)]
use std::f32::consts::TAU;
pub const RATE: u32 = 44_100;
pub const SECS: f32 = 180.0;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn noise(&mut self) -> f32 {
        self.next() as f32 / (1u64 << 31) as f32 - 1.0
    }
}


fn midi_hz(m: f32) -> f32 {
    440.0 * 2f32.powf((m - 69.0) / 12.0)
}

/// Semitone offsets of the chord roots and their quality (true = major triad).
fn progression(major: bool) -> [(i32, bool); 4] {
    if major {
        [(0, true), (7, true), (9, false), (5, true)] // I V vi IV
    } else {
        // Harmonic minor: the major V (leading tone) tells it from the relative major.
        [(0, false), (5, false), (7, true), (0, false)] // i iv V i
    }
}

pub fn synth(bpm: f32, tonic: u32, major: bool, seed: u64) -> Vec<f32> {
    let n = (SECS * RATE as f32) as usize;
    let mut out = vec![0f32; n];
    let mut rng = Rng(seed);
    let beat = 60.0 / bpm;
    // Drums.
    let mut b = 0usize;
    loop {
        let t0 = b as f32 * beat;
        if t0 >= SECS {
            break;
        }
        let i0 = (t0 * RATE as f32) as usize;
        // Kick: 55 Hz falling sine, 0.18 s.
        for k in 0..(0.18 * RATE as f32) as usize {
            let t = k as f32 / RATE as f32;
            if let Some(o) = out.get_mut(i0 + k) {
                *o += 0.7 * (-t / 0.05).exp() * (TAU * (55.0 + 60.0 * (-t / 0.02).exp()) * t).sin();
            }
        }
        // Snare on 2 and 4.
        if b % 2 == 1 {
            for k in 0..(0.12 * RATE as f32) as usize {
                let t = k as f32 / RATE as f32;
                if let Some(o) = out.get_mut(i0 + k) {
                    *o += 0.35 * (-t / 0.04).exp() * rng.noise();
                }
            }
        }
        // Hats on eighths (a crude high-pass: difference of noise).
        for half in 0..2 {
            let ih = i0 + (half as f32 * beat / 2.0 * RATE as f32) as usize;
            let mut prev = 0.0;
            for k in 0..(0.03 * RATE as f32) as usize {
                let t = k as f32 / RATE as f32;
                let v = rng.noise();
                if let Some(o) = out.get_mut(ih + k) {
                    *o += 0.12 * (-t / 0.01).exp() * (v - prev);
                }
                prev = v;
            }
        }
        b += 1;
    }
    // Harmony: one chord per bar (4 beats), bass on the root.
    let bar = 4.0 * beat;
    let prog = progression(major);
    for (i, o) in out.iter_mut().enumerate() {
        let t = i as f32 / RATE as f32;
        let bar_i = (t / bar) as usize;
        let (root, maj) = prog[bar_i % 4];
        let tb = t - bar_i as f32 * bar;
        let env = (tb / 0.02).min(1.0) * (1.0 - (tb / bar)).max(0.0).powf(0.3);
        let base = 60.0 + tonic as f32 + root as f32;
        let third = if maj { 4.0 } else { 3.0 };
        let mut v = 0.0;
        for (step, amp) in [(0.0, 0.10), (third, 0.08), (7.0, 0.08)] {
            v += amp * (TAU * midi_hz(base + step) * t).sin();
        }
        v += 0.12 * (TAU * midi_hz(base - 24.0) * t).sin();
        *o += env * v;
    }
    let peak = out.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-6);
    for o in &mut out {
        *o *= 0.9 / peak;
    }
    out
}

