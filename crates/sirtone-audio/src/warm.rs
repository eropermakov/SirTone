//! «Тёплый звук» of the sleep timer (T4.21, FR-SLEEP-004, `04` §7, `14` §14): a gentle RBJ
//! low-pass (Q 0.707) before the limiter whose cut-off glides 20 kHz → 6 kHz through the fade.
//! The glide is on a log-frequency scale, coefficients per 64-frame block. Off — a true bypass.

const Q: f64 = std::f64::consts::FRAC_1_SQRT_2;
const BLOCK: usize = 64;
/// One block moves this share of the way (in log frequency) to the target: ≈ 50 ms at 48 kHz.
const GLIDE: f64 = 0.06;

#[derive(Clone, Copy)]
struct Coefs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

fn lowpass(rate: f64, f: f64) -> Coefs {
    let f = f.clamp(20.0, rate * 0.45);
    let w = 2.0 * std::f64::consts::PI * f / rate;
    let (s, c) = w.sin_cos();
    let alpha = s / (2.0 * Q);
    let a0 = 1.0 + alpha;
    Coefs {
        b0: (1.0 - c) / 2.0 / a0,
        b1: (1.0 - c) / a0,
        b2: (1.0 - c) / 2.0 / a0,
        a1: -2.0 * c / a0,
        a2: (1.0 - alpha) / a0,
    }
}

pub struct Warm {
    rate: f64,
    channels: usize,
    /// ln of the cut-off now and its target.
    now: f64,
    to: f64,
    coefs: Coefs,
    /// (x1, x2, y1, y2) per channel.
    state: Vec<[f64; 4]>,
    on: bool,
}

/// Where the glide starts and where «off» returns before the bypass.
pub const OPEN_HZ: f32 = 20_000.0;

impl Warm {
    pub fn new(rate: u32, channels: usize) -> Self {
        let rate = f64::from(rate);
        let open = f64::from(OPEN_HZ).ln();
        Self {
            rate,
            channels: channels.max(1),
            now: open,
            to: open,
            coefs: lowpass(rate, f64::from(OPEN_HZ)),
            state: vec![[0.0; 4]; channels.max(1)],
            on: false,
        }
    }

    /// `Some(hz)` — glide to this cut-off (switching on from fully open); `None` — off.
    pub fn set(&mut self, cutoff: Option<f32>) {
        match cutoff {
            Some(hz) => {
                if !self.on {
                    self.on = true;
                    self.now = f64::from(OPEN_HZ).ln();
                    self.coefs = lowpass(self.rate, f64::from(OPEN_HZ));
                    self.state.fill([0.0; 4]);
                }
                self.to = f64::from(hz.clamp(1000.0, OPEN_HZ)).ln();
            }
            None => self.on = false,
        }
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Filters interleaved `buf` in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        if !self.on {
            return;
        }
        let ch = self.channels;
        for block in buf.chunks_mut(BLOCK * ch) {
            if (self.now - self.to).abs() > 1e-4 {
                self.now += (self.to - self.now) * GLIDE;
                self.coefs = lowpass(self.rate, self.now.exp());
            }
            let k = self.coefs;
            for frame in block.chunks_exact_mut(ch) {
                for (s, st) in frame.iter_mut().zip(self.state.iter_mut()) {
                    let x = f64::from(*s);
                    let y = k.b0 * x + k.b1 * st[0] + k.b2 * st[1] - k.a1 * st[2] - k.a2 * st[3];
                    *st = [x, st[0], y, st[2]];
                    *s = y as f32;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f64, rate: f64, secs: f64) -> Vec<f32> {
        let n = (rate * secs) as usize;
        (0..n)
            .flat_map(|i| {
                let v = (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / rate).sin()) as f32;
                [v, v]
            })
            .collect()
    }

    fn rms_db(x: &[f32]) -> f64 {
        let half = &x[x.len() / 2..];
        let p: f64 = half.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / half.len() as f64;
        10.0 * p.log10()
    }

    #[test]
    fn off_is_bit_exact() {
        let mut w = Warm::new(48_000, 2);
        let x = tone(9000.0, 48_000.0, 0.2);
        let mut y = x.clone();
        w.process(&mut y);
        assert_eq!(x, y);
        w.set(Some(6000.0));
        w.set(None);
        let mut z = x.clone();
        w.process(&mut z);
        assert_eq!(x, z);
    }

    #[test]
    fn at_six_khz_highs_are_softened_lows_kept() {
        for (hz, lo, hi) in [(1000.0, -0.3, 0.1), (14_000.0, -40.0, -10.0)] {
            let mut w = Warm::new(48_000, 2);
            w.set(Some(6000.0));
            let x = tone(hz, 48_000.0, 2.0);
            let mut y = x.clone();
            w.process(&mut y);
            let d = rms_db(&y) - rms_db(&x);
            assert!((lo..=hi).contains(&d), "{hz} Hz: {d:.2} dB");
        }
    }
}
