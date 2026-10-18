//! S-08 (часть A): `stratum-dsp` 1.0 — темп и тональность на корпусе с известными ответами.
//!
//! Корпус синтезируется здесь же (детерминированно, без файлов): ударные (бочка на каждую долю,
//! малый на 2 и 4, хэт на восьмые) в заданном темпе + аккорды по такту и бас в заданной
//! тональности (мажор I–V–vi–IV, минор i–iv–V–i гармонический). 48 треков по 40 с: темпы 62…188 BPM,
//! все 24 тональности по два раза. Три режима: по умолчанию; без мультиразрешения и band fusion;
//! облегчённый по первой минуте (как «быстрый анализ» Mixxx). Считаем: BPM ±2 как есть и после приведения к 70–180
//! (×2/÷2, как диапазон Traktor/djay), октавные ошибки, тональность — точно / относительная /
//! соседняя по кварто-квинтовому кругу, долю и точность при уверенности ≥ 0,8, время на трек.
//!
//! `cargo run --release` (CARGO_TARGET_DIR=target/spikes).

use std::f32::consts::TAU;
use std::time::Instant;

use stratum_dsp::{AnalysisConfig, Key, analyze_audio};

const RATE: u32 = 44_100;
const SECS: f32 = 180.0;

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

fn synth(bpm: f32, tonic: u32, major: bool, seed: u64) -> Vec<f32> {
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

/// Standard Camelot (8B = C major, 8A = A minor).
fn camelot(tonic: u32, major: bool) -> String {
    // Camelot number of the major key with this tonic: C=8, G=9, D=10, … (each fifth +1).
    let fifths = (tonic * 7) % 12; // position on the circle of fifths from C
    if major {
        format!("{}B", (fifths + 7) % 12 + 1)
    } else {
        // The relative major is 3 semitones up.
        let rel = (tonic + 3) % 12;
        format!("{}A", ((rel * 7) % 12 + 7) % 12 + 1)
    }
}

fn key_of(k: &Key) -> (u32, bool) {
    match k {
        Key::Major(i) => (*i % 12, true),
        Key::Minor(i) => (*i % 12, false),
    }
}

/// Into 70–180 by doubling/halving (the range approach of Traktor/djay).
fn fold(mut bpm: f32) -> f32 {
    while bpm > 0.0 && bpm < 70.0 {
        bpm *= 2.0;
    }
    while bpm > 180.0 {
        bpm /= 2.0;
    }
    bpm
}

fn main() {
    let mut rows = Vec::new();
    let mut seed = 7u64;
    for i in 0..48u32 {
        let bpm = 62.0 + (i as f32 * 2.63) % 126.0 + (i % 3) as f32 * 0.3;
        let tonic = i % 12;
        let major = (i / 12) % 2 == 0;
        seed = seed.wrapping_mul(31).wrapping_add(i as u64);
        let x = synth(bpm, tonic, major, seed);
        let mode = std::env::args().nth(1).unwrap_or_default();
        let mut cfg = AnalysisConfig::default();
        let mut part: &[f32] = &x;
        if mode == "light" || mode == "minute" {
            cfg.enable_tempogram_multi_resolution = false;
            cfg.enable_tempogram_band_fusion = false;
        }
        if mode == "light" || mode == "minute" || mode == "minute22" || mode == "half" {
            cfg.enable_tempogram_multi_resolution = false;
            cfg.enable_tempogram_band_fusion = false;
        }
        if mode == "minute" || mode == "minute22" {
            // The first minute after the intro bars (like Mixxx fast analysis).
            part = &x[(10 * RATE) as usize..(70 * RATE) as usize];
        }
        if mode == "half" {
            part = &x[(10 * RATE) as usize..(40 * RATE) as usize];
        }
        let down: Vec<f32>;
        let mut rate = RATE;
        if mode == "minute22" {
            // ÷2 with a two-tap average (the analysis looks at < 5 kHz anyway).
            down = part.chunks_exact(2).map(|p| 0.5 * (p[0] + p[1])).collect();
            part = &down;
            rate = RATE / 2;
        }
        let t = Instant::now();
        let r = analyze_audio(part, rate, cfg).expect("analysed");
        let ms = t.elapsed().as_secs_f64() * 1e3;
        rows.push((bpm, tonic, major, r.bpm, r.bpm_confidence, key_of(&r.key), r.key_confidence, ms, r.grid_stability, r.key_clarity));
    }
    let n = rows.len() as f32;
    let within = |a: f32, b: f32| (a - b).abs() <= 2.0;
    let raw = rows.iter().filter(|r| within(r.3, r.0)).count();
    let folded = rows.iter().filter(|r| within(fold(r.3), fold(r.0))).count();
    let up_to_octave = rows
        .iter()
        .filter(|r| within(r.3, r.0) || within(r.3 * 2.0, r.0) || within(r.3 / 2.0, r.0))
        .count();
    let octave = rows
        .iter()
        .filter(|r| !within(r.3, r.0) && (within(r.3 * 2.0, r.0) || within(r.3 / 2.0, r.0)))
        .count();
    let conf_rows: Vec<_> = rows.iter().filter(|r| r.4 >= 0.8).collect();
    let conf_ok = conf_rows.iter().filter(|r| within(fold(r.3), fold(r.0))).count();
    let key_exact = rows.iter().filter(|r| r.5 == (r.1, r.2)).count();
    let key_relative = rows
        .iter()
        .filter(|r| {
            let (t, m) = r.5;
            m != r.2 && (if r.2 { t == (r.1 + 9) % 12 } else { t == (r.1 + 3) % 12 })
        })
        .count();
    let key_fifth = rows
        .iter()
        .filter(|r| r.5.1 == r.2 && (r.5.0 == (r.1 + 7) % 12 || r.5.0 == (r.1 + 5) % 12))
        .count();
    let kc: Vec<_> = rows.iter().filter(|r| r.6 >= 0.3).collect();
    let kc_ok = kc.iter().filter(|r| r.5 == (r.1, r.2)).count();
    let mut ms: Vec<f64> = rows.iter().map(|r| r.7).collect();
    ms.sort_by(f64::total_cmp);
    println!("S-08: {} синтетических треков по {SECS} с, 44,1 кГц моно", rows.len());
    println!("BPM ±2 как есть:            {:.1} % ({raw})", raw as f32 / n * 100.0);
    println!("BPM ±2 после 70–180 (×2/÷2): {:.1} % ({folded})", folded as f32 / n * 100.0);
    println!("октавные ошибки (×2/÷2):    {octave}");
    println!("верно с точностью до октавы: {:.1} % ({up_to_octave})", up_to_octave as f32 / n * 100.0);
    println!(
        "уверенность ≥ 0,8: {} треков ({:.0} %), из них верно после 70–180: {:.1} %",
        conf_rows.len(),
        conf_rows.len() as f32 / n * 100.0,
        if conf_rows.is_empty() { 0.0 } else { conf_ok as f32 / conf_rows.len() as f32 * 100.0 }
    );
    println!(
        "тональность точно: {:.1} % ({key_exact}); относительная: {key_relative}; соседняя квинта: {key_fifth}",
        key_exact as f32 / n * 100.0
    );
    println!(
        "уверенность ключа ≥ 0,3: {} треков, из них точно {:.1} %",
        kc.len(),
        if kc.is_empty() { 0.0 } else { kc_ok as f32 / kc.len() as f32 * 100.0 }
    );
    println!(
        "время на трек: медиана {:.0} мс, max {:.0} мс",
        ms[ms.len() / 2],
        ms[ms.len() - 1]
    );
    println!("\nпримеры (BPM ист → найдено / conf; ключ ист → найдено / conf):");
    for r in rows.iter().take(if std::env::var_os("S08_ROWS").is_some() { 48 } else { 0 }) {
        println!(
            "  {:6.1} → {:6.1} / {:.2} grid {:.2}   {} → {} / {:.2} clarity {:.2}",
            r.0,
            r.3,
            r.4,
            r.8,
            camelot(r.1, r.2),
            camelot(r.5.0, r.5.1),
            r.6,
            r.9
        );
    }
}

#[cfg(test)]
mod tests {
    use super::camelot;
    #[test]
    fn camelot_standard() {
        assert_eq!(camelot(0, true), "8B"); // C
        assert_eq!(camelot(9, false), "8A"); // Am
        assert_eq!(camelot(7, true), "9B"); // G
        assert_eq!(camelot(4, false), "9A"); // Em
        assert_eq!(camelot(5, true), "7B"); // F
        assert_eq!(camelot(2, false), "7A"); // Dm
    }
}
