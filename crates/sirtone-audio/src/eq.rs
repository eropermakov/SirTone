//! Graphic equalizer (T4.12, FR-PLAY-009, `04` §6 — copy of F-03: Winamp/VLC 10 bands + preamp,
//! Poweramp/AIMP presets, true bypass). RBJ peaking biquads (Audio EQ Cookbook) on the ISO
//! octave centres 31.25 Hz … 16 kHz, Q ≈ 1.41, ±12 dB, preamp ±12 dB plus an automatic
//! anti-clip preamp: minus the peak of the whole curve (D-98; `04` §6 says `−max band`, but the
//! bands overlap and that is not enough). Coefficients and state in `f64`; a change is
//! interpolated over 20 ms (no zipper noise). Runs on the engine thread after the resampler,
//! before the ring; off and settled = true bypass (bit-identical).

/// ISO octave centres (Hz).
pub const FREQS: [f64; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
];
pub const Q: f64 = 1.41;
pub const MAX_DB: f32 = 12.0;
/// Coefficient interpolation time and step (samples between coefficient updates).
const GLIDE_MS: f64 = 20.0;
const STEP: usize = 32;

/// What the user set (dB).
#[derive(Debug, Clone, PartialEq)]
pub struct EqParams {
    pub enabled: bool,
    pub preamp_db: f32,
    pub bands_db: Vec<f32>,
}

impl Default for EqParams {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            bands_db: vec![0.0; FREQS.len()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Coefs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

const UNITY: Coefs = Coefs {
    b0: 1.0,
    b1: 0.0,
    b2: 0.0,
    a1: 0.0,
    a2: 0.0,
};

/// RBJ peaking EQ, normalised by a0.
fn peaking(rate: f64, f0: f64, q: f64, db: f64) -> Coefs {
    if db.abs() < 1e-9 || f0 >= rate * 0.5 {
        return UNITY;
    }
    let a = 10f64.powf(db / 40.0);
    let w0 = std::f64::consts::TAU * f0 / rate;
    let alpha = w0.sin() / (2.0 * q);
    let cos = w0.cos();
    let a0 = 1.0 + alpha / a;
    Coefs {
        b0: (1.0 + alpha * a) / a0,
        b1: (-2.0 * cos) / a0,
        b2: (1.0 - alpha * a) / a0,
        a1: (-2.0 * cos) / a0,
        a2: (1.0 - alpha / a) / a0,
    }
}

/// |H| of a biquad at `f` Hz, dB.
fn response_db(k: &Coefs, rate: f64, f: f64) -> f64 {
    let w = std::f64::consts::TAU * f / rate;
    let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
    let nr = k.b0 + k.b1 * c1 + k.b2 * c2;
    let ni = -(k.b1 * s1 + k.b2 * s2);
    let dr = 1.0 + k.a1 * c1 + k.a2 * c2;
    let di = -(k.a1 * s1 + k.a2 * s2);
    10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
}

/// The loudest point of the whole curve (bands overlap: +12 dB on every band is ≈ +18 dB in
/// the middle), on a log grid 20 Hz … 20 kHz.
fn curve_peak_db(bands: &[Coefs], rate: f64) -> f64 {
    (0..96)
        .map(|i| 20.0 * 1000f64.powf(f64::from(i) / 95.0))
        .filter(|f| *f < rate * 0.5)
        .map(|f| bands.iter().map(|k| response_db(k, rate, f)).sum::<f64>())
        .fold(0.0, f64::max)
}

fn lerp(a: Coefs, b: Coefs, t: f64) -> Coefs {
    let l = |x: f64, y: f64| x + (y - x) * t;
    Coefs {
        b0: l(a.b0, b.b0),
        b1: l(a.b1, b.b1),
        b2: l(a.b2, b.b2),
        a1: l(a.a1, b.a1),
        a2: l(a.a2, b.a2),
    }
}

pub struct Eq {
    rate: f64,
    channels: usize,
    /// Coefficients now, the glide's start and its target, per band.
    now: Vec<Coefs>,
    from: Vec<Coefs>,
    to: Vec<Coefs>,
    gain_now: f64,
    gain_from: f64,
    gain_to: f64,
    /// Samples (frames) into the glide, and its length.
    t: usize,
    len: usize,
    /// Biquad state per band and channel: (x1, x2, y1, y2).
    state: Vec<[f64; 4]>,
    /// Off and settled: samples pass untouched.
    bypass: bool,
    enabled: bool,
}

impl Eq {
    pub fn new(rate: u32, channels: usize) -> Self {
        let n = FREQS.len();
        Self {
            rate: f64::from(rate),
            channels: channels.max(1),
            now: vec![UNITY; n],
            from: vec![UNITY; n],
            to: vec![UNITY; n],
            gain_now: 1.0,
            gain_from: 1.0,
            gain_to: 1.0,
            t: 0,
            len: 0,
            state: vec![[0.0; 4]; n * channels.max(1)],
            bypass: true,
            enabled: false,
        }
    }

    /// New settings; glides from what sounds now. Off glides to flat, then bypasses.
    pub fn set(&mut self, p: &EqParams) {
        let clamp = |v: f32| f64::from(v.clamp(-MAX_DB, MAX_DB));
        let bands: Vec<f64> = (0..FREQS.len())
            .map(|i| {
                if p.enabled {
                    clamp(p.bands_db.get(i).copied().unwrap_or(0.0))
                } else {
                    0.0
                }
            })
            .collect();
        self.from.clone_from(&self.now);
        self.gain_from = self.gain_now;
        self.to = FREQS
            .iter()
            .zip(&bands)
            .map(|(f, db)| peaking(self.rate, *f, Q, *db))
            .collect();
        let auto = -curve_peak_db(&self.to, self.rate);
        let pre = if p.enabled {
            clamp(p.preamp_db) + auto
        } else {
            0.0
        };
        self.gain_to = 10f64.powf(pre / 20.0);
        self.t = 0;
        self.len = ((self.rate * GLIDE_MS / 1000.0) as usize).max(1);
        self.enabled = p.enabled;
        if self.bypass && p.enabled {
            // Leaving bypass: the filters start from silence-free zero state.
            self.state.fill([0.0; 4]);
        }
        self.bypass = false;
    }

    /// Filters interleaved `buf` in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        if self.bypass {
            return;
        }
        let ch = self.channels;
        let frames = buf.len() / ch;
        let mut f = 0;
        while f < frames {
            // Coefficients for the next block.
            if self.t < self.len {
                let k = (self.t as f64 / self.len as f64).min(1.0);
                for b in 0..self.now.len() {
                    self.now[b] = lerp(self.from[b], self.to[b], k);
                }
                self.gain_now = self.gain_from + (self.gain_to - self.gain_from) * k;
            } else {
                self.now.clone_from(&self.to);
                self.gain_now = self.gain_to;
            }
            let end = (f + STEP).min(frames);
            for fr in f..end {
                for c in 0..ch {
                    let i = fr * ch + c;
                    let mut x = f64::from(buf[i]) * self.gain_now;
                    for (b, k) in self.now.iter().enumerate() {
                        if *k == UNITY {
                            continue;
                        }
                        let s = &mut self.state[b * ch + c];
                        let y = k.b0 * x + k.b1 * s[0] + k.b2 * s[1] - k.a1 * s[2] - k.a2 * s[3];
                        s[1] = s[0];
                        s[0] = x;
                        s[3] = s[2];
                        s[2] = y;
                        x = y;
                    }
                    buf[i] = x as f32;
                }
            }
            self.t += end - f;
            f = end;
        }
        if !self.enabled && self.t >= self.len {
            self.bypass = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, secs: f64, ch: usize) -> Vec<f32> {
        let n = (f64::from(rate) * secs) as usize;
        (0..n * ch)
            .map(|i| {
                (0.25 * (std::f64::consts::TAU * freq * (i / ch) as f64 / f64::from(rate)).sin())
                    as f32
            })
            .collect()
    }

    fn rms_db(x: &[f32]) -> f64 {
        let e: f64 = x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64;
        10.0 * e.log10()
    }

    /// The response at each band centre is the band's gain (one band set, the auto preamp
    /// taken off), and far bands are barely touched.
    #[test]
    fn band_centres_get_their_gain() {
        let rate = 48_000;
        for (i, f) in FREQS.iter().enumerate() {
            let mut eq = Eq::new(rate, 1);
            let mut bands = vec![0.0; 10];
            bands[i] = 6.0;
            eq.set(&EqParams {
                enabled: true,
                preamp_db: 6.0, // cancels the −6 dB auto preamp
                bands_db: bands,
            });
            let x = sine(*f, rate, 1.0, 1);
            let mut y = x.clone();
            eq.process(&mut y);
            let skip = 24_000;
            let d = rms_db(&y[skip..]) - rms_db(&x[skip..]);
            assert!((d - 6.0).abs() < 0.25, "{f} Hz: {d:.2} dB");
        }
        // A far band: 6 dB at 31 Hz leaves 4 kHz alone.
        let mut eq = Eq::new(rate, 1);
        let mut bands = vec![0.0; 10];
        bands[0] = 6.0;
        eq.set(&EqParams {
            enabled: true,
            preamp_db: 6.0,
            bands_db: bands,
        });
        let x = sine(4_000.0, rate, 0.5, 1);
        let mut y = x.clone();
        eq.process(&mut y);
        assert!((rms_db(&y[12_000..]) - rms_db(&x[12_000..])).abs() < 0.1);
    }

    /// Off is a true bypass; turning it off glides to flat and then passes samples untouched.
    #[test]
    fn off_is_bit_identical() {
        let rate = 44_100;
        let x = sine(1_000.0, rate, 0.2, 2);
        let mut eq = Eq::new(rate, 2);
        let mut y = x.clone();
        eq.process(&mut y);
        assert_eq!(x, y);
        eq.set(&EqParams {
            enabled: true,
            preamp_db: 0.0,
            bands_db: vec![3.0; 10],
        });
        let mut y = x.clone();
        eq.process(&mut y);
        assert_ne!(x, y);
        eq.set(&EqParams::default());
        let mut glide = x.clone();
        eq.process(&mut glide); // the 20 ms glide back to flat
        let mut y = x.clone();
        eq.process(&mut y);
        assert_eq!(x, y);
    }

    /// The auto preamp keeps a full-scale signal under 0 dBFS with boosted bands.
    #[test]
    fn auto_preamp_prevents_clipping() {
        let rate = 48_000;
        let mut eq = Eq::new(rate, 1);
        eq.set(&EqParams {
            enabled: true,
            preamp_db: 0.0,
            bands_db: vec![12.0; 10],
        });
        let x: Vec<f32> = sine(1_000.0, rate, 0.5, 1)
            .iter()
            .map(|v| v * 3.9)
            .collect();
        let mut y = x.clone();
        eq.process(&mut y);
        let peak = y[4_800..].iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak < 1.05, "{peak}");
    }
}
