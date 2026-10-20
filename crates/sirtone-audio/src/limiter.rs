//! Peak limiter (T4.13, FR-PLAY-002/008, `04` §7): lookahead 1.5 ms, ceiling −1 dBFS, release
//! 50 ms. The gain for a sample is the minimum needed over the lookahead window, reached with a
//! linear ramp across that window (so the reduction starts before the peak arrives) and released
//! exponentially; a last clamp at the ceiling catches what the ramp could not. Below the ceiling
//! it changes nothing but the delay: the samples come out bit-identical, 1.5 ms later.
//! Allocation-free after `new` (sliding-window minimum on fixed rings).

pub const LOOKAHEAD_MS: f64 = 1.5;
pub const CEILING_DB: f64 = -1.0;
pub const RELEASE_MS: f64 = 50.0;

pub struct Limiter {
    channels: usize,
    ceiling: f32,
    /// Delay line, `lookahead` frames × channels, a ring.
    delay: Vec<f32>,
    pos: usize,
    lookahead: usize,
    /// Required gains of the last `lookahead + 1` frames and a monotonic queue of their minimum
    /// (indices into `req`).
    req: Vec<f32>,
    mins: Vec<usize>,
    head: usize,
    len: usize,
    frame: usize,
    env: f32,
    release: f32,
}

impl Limiter {
    pub fn new(rate: u32, channels: usize) -> Self {
        let ch = channels.max(1);
        let lookahead = ((f64::from(rate) * LOOKAHEAD_MS / 1000.0).round() as usize).max(1);
        let window = lookahead + 1;
        Self {
            channels: ch,
            ceiling: 10f64.powf(CEILING_DB / 20.0) as f32,
            delay: vec![0.0; lookahead * ch],
            pos: 0,
            lookahead,
            req: vec![1.0; window],
            mins: vec![0; window],
            head: 0,
            len: 0,
            frame: 0,
            env: 1.0,
            release: 1.0 - (-1.0 / (f64::from(rate) * RELEASE_MS / 1000.0)).exp() as f32,
        }
    }

    /// The delay it adds, frames.
    pub fn latency(&self) -> usize {
        self.lookahead
    }

    /// Clears the delay line and the gain (a flush or a seek: nothing old must come out).
    pub fn reset(&mut self) {
        self.delay.fill(0.0);
        self.req.fill(1.0);
        self.pos = 0;
        self.head = 0;
        self.len = 0;
        self.env = 1.0;
    }

    /// Limits interleaved `buf` in place (the output is delayed by `latency()` frames).
    pub fn process(&mut self, buf: &mut [f32]) {
        let ch = self.channels;
        let w = self.req.len();
        for frame in buf.chunks_exact_mut(ch) {
            // The gain this incoming frame needs.
            let peak = frame.iter().fold(0f32, |m, s| m.max(s.abs()));
            let need = if peak > self.ceiling {
                self.ceiling / peak
            } else {
                1.0
            };
            let slot = self.frame % w;
            self.req[slot] = need;
            // Sliding minimum over the last `w` frames: drop larger tails, expired heads.
            while self.len > 0 {
                let last = self.mins[(self.head + self.len - 1) % w];
                if self.req[last % w] >= need {
                    self.len -= 1;
                } else {
                    break;
                }
            }
            self.mins[(self.head + self.len) % w] = self.frame;
            self.len += 1;
            while self.frame - self.mins[self.head] >= w {
                self.head = (self.head + 1) % w;
                self.len -= 1;
            }
            let target = self.req[self.mins[self.head] % w];
            // Down: a linear ramp that arrives within the lookahead; up: the release.
            if target < self.env {
                let step = (self.env - target) / self.lookahead as f32;
                self.env = (self.env - step).max(target);
            } else {
                self.env += (target - self.env) * self.release;
            }
            let env = self.env;
            for (c, s) in frame.iter_mut().enumerate() {
                let i = self.pos * ch + c;
                let out = self.delay[i];
                self.delay[i] = *s;
                *s = if env >= 1.0 {
                    out
                } else {
                    (out * env).clamp(-self.ceiling, self.ceiling)
                };
            }
            self.pos = (self.pos + 1) % self.lookahead;
            self.frame += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_the_ceiling_only_delays() {
        let mut l = Limiter::new(48_000, 2);
        let x: Vec<f32> = (0..9_600)
            .map(|i| 0.8 * ((i as f32) * 0.01).sin())
            .collect();
        let mut y = x.clone();
        l.process(&mut y);
        let d = l.latency() * 2;
        assert_eq!(&y[d..], &x[..x.len() - d]);
        assert!(y[..d].iter().all(|v| *v == 0.0));
    }

    #[test]
    fn peaks_never_pass_the_ceiling_and_the_gain_comes_back() {
        let rate = 48_000;
        let mut l = Limiter::new(rate, 1);
        let ceiling = 10f32.powf(-1.0 / 20.0);
        // Quiet, a hard burst at 2.0 (+6 dBFS), quiet again.
        let mut x = vec![0.1f32; 4_800];
        x.extend((0..2_400).map(|i| if i % 2 == 0 { 2.0 } else { -2.0 }));
        x.extend(vec![0.5f32; 48_000]);
        let mut y = x.clone();
        l.process(&mut y);
        let peak = y.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(peak <= ceiling + 1e-6, "{peak}");
        // The burst comes out at the ceiling, not crushed far below it.
        let d = l.latency();
        let burst = &y[4_800 + d + 200..4_800 + d + 2_400];
        assert!(
            burst.iter().all(|v| v.abs() > ceiling * 0.9),
            "over-limited"
        );
        // 300 ms later (6 release constants) the 0.5 level is back.
        let tail = y[4_800 + 2_400 + d + 14_400];
        assert!((tail - 0.5).abs() < 0.01, "{tail}");
    }
}
