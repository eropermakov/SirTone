//! S-08 part B (T4.9c): Signalsmith Stretch for the automix L3 tempo blend, and whether
//! stratum-dsp's beat grid is good enough to beatmatch on.
//!
//! `cargo run --release` prints:
//!   A. CPU of stretching 30 s of stereo 48 kHz at ×1.032 (124 → 128 BPM), real-time factor;
//!   B. beat alignment after the stretch: click track 124 BPM → 128 BPM, onset error vs the ideal grid;
//!   C. stratum grids on synthetic click tracks: steady / drifting / jittered — BPM, `grid_stability`,
//!      the phase error of its beats, to decide if `grid_stability` can gate L3.

mod corpus;

use std::ffi::c_void;
use std::time::Instant;

#[allow(non_camel_case_types)]
type handle = *mut c_void;
unsafe extern "C" {
    fn signalsmith_stretch_create_preset_default(channels: i32, rate: f32) -> handle;
    fn signalsmith_stretch_create_preset_cheaper(channels: i32, rate: f32) -> handle;
    fn signalsmith_stretch_destroy(h: handle);
    fn signalsmith_stretch_input_latency(h: handle) -> usize;
    fn signalsmith_stretch_output_latency(h: handle) -> usize;
    fn signalsmith_stretch_process(h: handle, input: *mut f32, in_len: usize, out: *mut f32, out_len: usize);
}

struct Stretch {
    h: handle,
    ch: usize,
}

impl Stretch {
    fn new(ch: usize, rate: u32, cheap: bool) -> Self {
        let h = unsafe {
            if cheap {
                signalsmith_stretch_create_preset_cheaper(ch as i32, rate as f32)
            } else {
                signalsmith_stretch_create_preset_default(ch as i32, rate as f32)
            }
        };
        Self { h, ch }
    }
    fn latency(&self) -> (usize, usize) {
        unsafe { (signalsmith_stretch_input_latency(self.h), signalsmith_stretch_output_latency(self.h)) }
    }
    fn process(&mut self, input: &mut [f32], out: &mut [f32]) {
        unsafe {
            signalsmith_stretch_process(
                self.h,
                input.as_mut_ptr(),
                input.len() / self.ch,
                out.as_mut_ptr(),
                out.len() / self.ch,
            )
        }
    }
}

impl Drop for Stretch {
    fn drop(&mut self) {
        unsafe { signalsmith_stretch_destroy(self.h) }
    }
}

const RATE: u32 = 48_000;

/// A click track: a 5 ms 2 kHz burst on every beat, a bass sine under it.
fn clicks(bpm: impl Fn(f64) -> f64, secs: f64, jitter_ms: f64, ch: usize) -> (Vec<f32>, Vec<f64>) {
    let n = (secs * RATE as f64) as usize;
    let mut x = vec![0f32; n * ch];
    let mut beats = Vec::new();
    let mut t = 0.5;
    let mut k = 0u64;
    while t < secs - 0.1 {
        // A deterministic «random» jitter.
        k = k.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = ((k >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 2.0 * jitter_ms / 1000.0;
        let at = t + j;
        beats.push(at);
        let s0 = (at * RATE as f64) as usize;
        for i in 0..(0.005 * RATE as f64) as usize {
            let v = (0.6 * (std::f64::consts::TAU * 2000.0 * i as f64 / RATE as f64).sin()
                * (1.0 - i as f64 / 240.0)) as f32;
            for c in 0..ch {
                if let Some(s) = x.get_mut((s0 + i) * ch + c) {
                    *s += v;
                }
            }
        }
        t += 60.0 / bpm(t);
    }
    for i in 0..n {
        let v = (0.1 * (std::f64::consts::TAU * 55.0 * i as f64 / RATE as f64).sin()) as f32;
        for c in 0..ch {
            x[i * ch + c] += v;
        }
    }
    (x, beats)
}

/// Onsets: rising crossings of a short-term peak above half the maximum, 100 ms dead time.
fn onsets(x: &[f32], ch: usize) -> Vec<f64> {
    let mono: Vec<f32> = x.chunks(ch).map(|f| f[0].abs()).collect();
    let mx = mono.iter().cloned().fold(0f32, f32::max);
    let thr = mx * 0.5;
    let mut out = Vec::new();
    let mut last = -1.0;
    for (i, v) in mono.iter().enumerate() {
        let t = i as f64 / RATE as f64;
        if *v > thr && t - last > 0.1 {
            out.push(t);
            last = t;
        }
    }
    out
}

fn stretch_all(x: &[f32], ch: usize, ratio: f64, cheap: bool) -> (Vec<f32>, f64) {
    let mut s = Stretch::new(ch, RATE, cheap);
    let (lin, lout) = s.latency();
    let block_out = 512usize;
    let mut out = Vec::new();
    let mut pos = 0usize;
    let n_in = x.len() / ch;
    let t0 = Instant::now();
    let mut ob = vec![0f32; block_out * ch];
    let mut acc = 0.0f64;
    while pos < n_in {
        acc += block_out as f64 * ratio;
        let take = (acc as usize).min(n_in - pos);
        acc -= take as f64;
        let mut ib = x[pos * ch..(pos + take) * ch].to_vec();
        s.process(&mut ib, &mut ob);
        out.extend_from_slice(&ob);
        pos += take;
    }
    let el = t0.elapsed().as_secs_f64();
    // Drop the latency (input + output) from the start.
    let skip = ((lin as f64 / ratio) as usize + lout) * ch;
    (out[skip.min(out.len())..].to_vec(), el)
}

/// Own beat grid (T4.9c): an onset-strength envelope (log energy rise, hop 256) and a comb over
/// the beat period near `bpm_hint` (±3 %, ×1): the period and phase that collect the most onset
/// strength. Stability = how much the best phase stands out from the average phase (0…1).
fn own_grid(x: &[f32], bpm_hint: f64) -> (f64, f64, f64) {
    own_grid_at(x, RATE, bpm_hint)
}

fn own_grid_at(x: &[f32], rate: u32, bpm_hint: f64) -> (f64, f64, f64) {
    let hop = 256usize;
    let n = x.len() / hop;
    let mut e = vec![0f64; n];
    for (i, v) in e.iter_mut().enumerate() {
        let s: f64 = x[i * hop..(i + 1) * hop].iter().map(|v| f64::from(*v).powi(2)).sum();
        *v = (1e-9 + s).ln();
    }
    let mut flux = vec![0f64; n];
    for i in 1..n {
        flux[i] = (e[i] - e[i - 1]).max(0.0);
    }
    let fps = rate as f64 / hop as f64;
    let at = |t: f64| -> f64 {
        let i = t.floor() as usize;
        if i + 1 >= n {
            return 0.0;
        }
        let f = t - i as f64;
        flux[i] * (1.0 - f) + flux[i + 1] * f
    };
    let mut best = (0.0, 0.0, 0.0, 0.0); // score, period, phase, mean
    for k in -30..=30 {
        let bpm = bpm_hint * (1.0 + k as f64 * 0.001);
        let period = fps * 60.0 / bpm;
        let steps = (period * 4.0) as usize;
        let mut scores = Vec::with_capacity(steps);
        for p in 0..steps {
            let phase = p as f64 / 4.0;
            let mut sum = 0.0;
            let mut c = 0;
            let mut t = phase;
            while t < n as f64 - 1.0 {
                sum += at(t);
                c += 1;
                t += period;
            }
            scores.push((sum / c.max(1) as f64, phase));
        }
        let mean = scores.iter().map(|s| s.0).sum::<f64>() / scores.len() as f64;
        let (sc, ph) = scores.iter().cloned().fold((0.0, 0.0), |a, b| if b.0 > a.0 { b } else { a });
        if sc > best.0 {
            best = (sc, period, ph, mean);
        }
    }
    let (sc, period, phase, mean) = best;
    let stability = if sc > 0.0 { 1.0 - mean / sc } else { 0.0 };
    (fps * 60.0 / period, phase / fps, stability)
}

fn main() {
    let ch = 2;
    // A. CPU.
    let (x, _) = clicks(|_| 124.0, 30.0, 0.0, ch);
    for cheap in [false, true] {
        let (y, el) = stretch_all(&x, ch, 128.0 / 124.0, cheap);
        let secs = y.len() as f64 / ch as f64 / RATE as f64;
        println!(
            "A. {} preset: {:.2} s for {:.1} s of output → {:.1} % of one core (real-time ×{:.0})",
            if cheap { "cheaper" } else { "default" },
            el,
            secs,
            100.0 * el / secs,
            secs / el
        );
    }
    // B. Beat alignment after the stretch: 124 → 128 BPM.
    let (y, _) = stretch_all(&x, ch, 128.0 / 124.0, false);
    let on = onsets(&y, ch);
    let period = 60.0 / 128.0;
    let first = on.first().copied().unwrap_or(0.0);
    let errs: Vec<f64> = on
        .iter()
        .map(|t| {
            let k = ((t - first) / period).round();
            (t - first - k * period).abs() * 1000.0
        })
        .collect();
    let max = errs.iter().cloned().fold(0.0, f64::max);
    let mean = errs.iter().sum::<f64>() / errs.len().max(1) as f64;
    println!(
        "B. stretched 124→128: {} onsets, beat error vs the 128 BPM grid mean {:.1} ms, max {:.1} ms (threshold 20 ms)",
        on.len(),
        mean,
        max
    );
    // C. stratum grids.
    let cfg = || stratum_dsp::AnalysisConfig {
        enable_tempogram_multi_resolution: false,
        enable_tempogram_band_fusion: false,
        ..stratum_dsp::AnalysisConfig::default()
    };
    // C2. The own grid on the part A drum corpus: the tail 30 s, phase vs the true beats (the
    // first beat at 0, then every 60/bpm).
    let mut ok = 0;
    let mut stabs = Vec::new();
    for (i, bpm) in [62.0f32, 75.0, 90.0, 100.0, 110.0, 124.0, 128.0, 140.0, 150.0, 174.0, 188.0]
        .iter()
        .enumerate()
    {
        let x = corpus::synth(*bpm, (i % 12) as u32, i % 2 == 0, i as u64 + 7);
        let r = corpus::RATE as usize;
        let tail = &x[x.len() - 30 * r..];
        let t0 = (x.len() - 30 * r) as f64 / r as f64;
        let hint = if *bpm > 180.0 { *bpm as f64 / 2.0 } else { *bpm as f64 };
        let (ob, ph, st) = own_grid_at(tail, corpus::RATE, hint);
        let period = 60.0 / *bpm as f64;
        let abs = t0 + ph;
        let e = ((abs / period) - (abs / period).round()).abs() * period * 1000.0;
        // The grid at the hint's tempo: a beat — or, at ×2/÷2, a half beat is still on the grid.
        if e <= 20.0 {
            ok += 1;
        }
        stabs.push(st);
        println!("C2. drums {bpm:5.1} → {ob:6.2} stability {st:.2} phase err {e:5.1} ms");
    }
    println!(
        "C2. drums: {ok}/11 within 20 ms; stability min {:.2}",
        stabs.iter().cloned().fold(1.0, f64::min)
    );
    let cases: Vec<(&str, Box<dyn Fn(f64) -> f64>, f64)> = vec![
        ("steady 124", Box::new(|_| 124.0), 0.0),
        ("steady 90", Box::new(|_| 90.0), 0.0),
        ("steady 174", Box::new(|_| 174.0), 0.0),
        ("drift 120→132", Box::new(|t| 120.0 + 12.0 * t / 30.0), 0.0),
        ("jitter ±25 ms", Box::new(|_| 124.0), 25.0),
        ("jitter ±60 ms", Box::new(|_| 124.0), 60.0),
    ];
    for (name, bpm, jitter) in cases {
        let (x, truth) = clicks(bpm, 30.0, jitter, 1);
        let t0 = Instant::now();
        let r = stratum_dsp::analyze_audio(&x, RATE, cfg()).expect("analysed");
        let el = t0.elapsed().as_millis();
        let beats: Vec<f64> = r.beat_grid.beats.iter().map(|b| *b as f64).collect();
        // Phase error: each detected beat vs the nearest true beat.
        let err: Vec<f64> = beats
            .iter()
            .map(|b| truth.iter().map(|t| (t - b).abs()).fold(f64::MAX, f64::min) * 1000.0)
            .collect();
        let mean = err.iter().sum::<f64>() / err.len().max(1) as f64;
        let hint = if r.bpm > 0.0 { r.bpm as f64 } else { 120.0 };
        let (ob, ophase, ostab) = own_grid(&x, hint);
        let pred: Vec<f64> = (0..200).map(|k| ophase + k as f64 * 60.0 / ob).filter(|t| *t < 30.0).collect();
        let oerr: Vec<f64> = truth
            .iter()
            .map(|t| pred.iter().map(|b| (t - b).abs()).fold(f64::MAX, f64::min) * 1000.0)
            .collect();
        println!(
            "   own grid: bpm {ob:6.2} stability {ostab:.2} phase err mean {:.1} ms max {:.1} ms",
            oerr.iter().sum::<f64>() / oerr.len().max(1) as f64,
            oerr.iter().cloned().fold(0.0, f64::max)
        );
        println!(
            "C. {name:14} bpm {:6.1} conf {:.2} grid_stability {:.2} beats {} (truth {}) phase err mean {:.1} ms max {:.1} ms ({el} ms)",
            r.bpm,
            r.bpm_confidence,
            r.grid_stability,
            beats.len(),
            truth.len(),
            mean,
            err.iter().cloned().fold(0.0, f64::max)
        );
    }
}
