//! Beat grid of a window (T4.9c, D-100 — S-08 part B): an onset-strength envelope (rise of the log
//! energy, hop 256) and a comb over the beat period near the tempo hint (±3 %, step 0.1 %): the
//! period and phase that collect the most onset strength. Stability = how far the best phase
//! stands out from the average one (steady drums ≈ 0.9, drifting/jittered ≤ 0.53; the automix
//! beatmatches at ≥ 0.6). stratum-dsp's own grid was 60–180 ms off and is not used.

/// A grid over a window: tempo, the first beat (seconds from the window start), stability 0…1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub bpm: f64,
    pub first_beat_s: f64,
    pub stability: f64,
}

const HOP: usize = 256;

/// `mono` at `rate`; `bpm_hint` from B1 (70–180). `None` — too short or silent.
pub fn estimate(mono: &[f32], rate: u32, bpm_hint: f64) -> Option<Grid> {
    let n = mono.len() / HOP;
    if n < 64 || bpm_hint <= 0.0 {
        return None;
    }
    let mut e = vec![0f64; n];
    for (i, v) in e.iter_mut().enumerate() {
        let s: f64 = mono[i * HOP..(i + 1) * HOP]
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum();
        *v = (1e-9 + s).ln();
    }
    let mut flux = vec![0f64; n];
    for i in 1..n {
        flux[i] = (e[i] - e[i - 1]).max(0.0);
    }
    let fps = f64::from(rate) / HOP as f64;
    let at = |t: f64| -> f64 {
        let i = t.floor() as usize;
        if i + 1 >= n {
            return 0.0;
        }
        let f = t - i as f64;
        flux[i] * (1.0 - f) + flux[i + 1] * f
    };
    // (score, period in hops, phase in hops, mean score over phases)
    let mut best = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for k in -30..=30 {
        let bpm = bpm_hint * (1.0 + f64::from(k) * 0.001);
        let period = fps * 60.0 / bpm;
        let steps = (period * 4.0) as usize;
        if steps == 0 {
            continue;
        }
        let (mut sum_all, mut top, mut top_phase) = (0.0, 0.0, 0.0);
        for p in 0..steps {
            let phase = p as f64 / 4.0;
            let (mut sum, mut c) = (0.0, 0u32);
            let mut t = phase;
            while t < n as f64 - 1.0 {
                sum += at(t);
                c += 1;
                t += period;
            }
            let score = sum / f64::from(c.max(1));
            sum_all += score;
            if score > top {
                top = score;
                top_phase = phase;
            }
        }
        if top > best.0 {
            best = (top, period, top_phase, sum_all / steps as f64);
        }
    }
    let (score, period, phase, mean) = best;
    if score <= 0.0 {
        return None;
    }
    Some(Grid {
        bpm: fps * 60.0 / period,
        first_beat_s: phase / fps,
        stability: 1.0 - mean / score,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clicks on a grid (optionally drifting), a bass under them.
    fn clicks(rate: u32, secs: f64, bpm: impl Fn(f64) -> f64, offset: f64) -> (Vec<f32>, Vec<f64>) {
        let n = (secs * f64::from(rate)) as usize;
        let mut x: Vec<f32> = (0..n)
            .map(|i| {
                (0.1 * (std::f64::consts::TAU * 55.0 * i as f64 / f64::from(rate)).sin()) as f32
            })
            .collect();
        let mut beats = Vec::new();
        let mut t = offset;
        while t < secs - 0.1 {
            beats.push(t);
            let s0 = (t * f64::from(rate)) as usize;
            for i in 0..(0.005 * f64::from(rate)) as usize {
                let v = 0.6 * (std::f64::consts::TAU * 2000.0 * i as f64 / f64::from(rate)).sin();
                if let Some(s) = x.get_mut(s0 + i) {
                    *s += v as f32;
                }
            }
            t += 60.0 / bpm(t);
        }
        (x, beats)
    }

    #[test]
    fn a_steady_grid_is_found_to_the_millisecond() {
        for bpm in [90.0, 124.0, 174.0] {
            let (x, truth) = clicks(48_000, 30.0, |_| bpm, 0.37);
            let g = estimate(&x, 48_000, bpm).unwrap();
            assert!((g.bpm - bpm).abs() < 0.2, "{bpm}: {g:?}");
            let period = 60.0 / g.bpm;
            let err = truth
                .iter()
                .map(|t| {
                    let k = ((t - g.first_beat_s) / period).round();
                    (t - g.first_beat_s - k * period).abs()
                })
                .fold(0.0, f64::max);
            assert!(err < 0.015, "{bpm}: phase error {err}");
            assert!(g.stability >= 0.6, "{bpm}: stability {}", g.stability);
        }
    }

    #[test]
    fn a_drifting_tempo_is_not_stable() {
        let (x, _) = clicks(48_000, 30.0, |t| 120.0 + 12.0 * t / 30.0, 0.5);
        let g = estimate(&x, 48_000, 126.0).unwrap();
        assert!(g.stability < 0.6, "{g:?}");
        assert_eq!(estimate(&[0.0; 100], 48_000, 120.0), None);
    }
}
