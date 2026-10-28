//! «Blend» — the EQ-swap of an L3 overlap (`04` §4.3; djay «EQ» transition, a DJ's bass kill —
//! `01a` §A): a bass cut is a 4th-order high-pass (Linkwitz–Riley, two RBJ stages at Q 0.707).
//! A plays whole until the swap point and then without its bass; B plays without its bass until
//! then and whole after. Each side moves between its plain and its cut signal over a 20 ms linear
//! ramp (no click); the cut filters run from the overlap's start, so they are warm by the swap.

const Q: f64 = std::f64::consts::FRAC_1_SQRT_2;
/// The swap's own ramp, s.
const RAMP_S: f64 = 0.02;

#[derive(Clone, Copy)]
struct Coefs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

fn highpass(rate: f64, f: f64) -> Coefs {
    let f = f.clamp(20.0, rate * 0.45);
    let w = 2.0 * std::f64::consts::PI * f / rate;
    let (s, c) = w.sin_cos();
    let alpha = s / (2.0 * Q);
    let a0 = 1.0 + alpha;
    Coefs {
        b0: (1.0 + c) / 2.0 / a0,
        b1: -(1.0 + c) / a0,
        b2: (1.0 + c) / 2.0 / a0,
        a1: -2.0 * c / a0,
        a2: (1.0 - alpha) / a0,
    }
}

/// Two stages, (x1, x2, y1, y2) each.
type Cut = [[f64; 4]; 2];

fn run(k: &Coefs, st: &mut Cut, x: f64) -> f64 {
    st.iter_mut().fold(x, |x, s| {
        let y = k.b0 * x + k.b1 * s[0] + k.b2 * s[1] - k.a1 * s[2] - k.a2 * s[3];
        *s = [x, s[0], y, s[2]];
        y
    })
}

pub(crate) struct EqSwap {
    coefs: Coefs,
    /// The swap's middle and half its ramp, overlap frames.
    at: f64,
    half: f64,
    /// Per channel, A and B.
    a: Vec<Cut>,
    b: Vec<Cut>,
}

impl EqSwap {
    pub(crate) fn new(rate: u32, channels: usize, hz: f32, at_frames: u64) -> Self {
        let rate = f64::from(rate);
        Self {
            coefs: highpass(rate, f64::from(hz)),
            at: at_frames as f64,
            half: RAMP_S * rate / 2.0,
            a: vec![[[0.0; 4]; 2]; channels.max(1)],
            b: vec![[[0.0; 4]; 2]; channels.max(1)],
        }
    }

    /// How far the swap has gone at overlap frame `pos`: 0 → 1 over the ramp.
    fn share(&self, pos: u64) -> f64 {
        ((pos as f64 - (self.at - self.half)) / (2.0 * self.half)).clamp(0.0, 1.0)
    }

    /// One frame of channel `c`: A's and B's samples with the bass handed over.
    pub(crate) fn apply(&mut self, c: usize, pos: u64, a: f32, b: f32) -> (f32, f32) {
        let k = self.share(pos);
        let (xa, xb) = (f64::from(a), f64::from(b));
        let ca = run(&self.coefs, &mut self.a[c], xa);
        let cb = run(&self.coefs, &mut self.b[c], xb);
        (
            (xa * (1.0 - k) + ca * k) as f32,
            (cb * (1.0 - k) + xb * k) as f32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    fn rms(xs: &[f32]) -> f64 {
        (xs.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / xs.len() as f64).sqrt()
    }

    fn tone(hz: f64, i: usize) -> f32 {
        (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / f64::from(RATE)).sin()) as f32
    }

    /// A 1 s overlap swapped at 0.5 s: what A and B give out for tones `ha` and `hb`.
    fn through(ha: f64, hb: f64) -> (Vec<f32>, Vec<f32>) {
        let n = RATE as usize;
        let mut s = EqSwap::new(RATE, 1, 200.0, (n / 2) as u64);
        (0..n)
            .map(|i| s.apply(0, i as u64, tone(ha, i), tone(hb, i + 123)))
            .unzip()
    }

    #[test]
    fn bass_hands_over_at_the_swap() {
        let (a, b) = through(60.0, 60.0);
        let q = RATE as usize / 4;
        // Before the swap: A's bass whole, B's cut; after it the other way round.
        assert!(
            rms(&a[q..2 * q - 1000]) > 0.34,
            "{}",
            rms(&a[q..2 * q - 1000])
        );
        assert!(
            rms(&b[q..2 * q - 1000]) < 0.02,
            "{}",
            rms(&b[q..2 * q - 1000])
        );
        assert!(rms(&a[3 * q..]) < 0.02, "{}", rms(&a[3 * q..]));
        assert!(rms(&b[3 * q..]) > 0.34, "{}", rms(&b[3 * q..]));
    }

    #[test]
    fn the_rest_passes_on_both_sides() {
        let (a, b) = through(3000.0, 3000.0);
        let q = RATE as usize / 4;
        for x in [&a[q..2 * q], &a[3 * q..], &b[q..2 * q], &b[3 * q..]] {
            assert!(rms(x) > 0.34, "{}", rms(x));
        }
    }

    #[test]
    fn a_is_untouched_before_the_ramp() {
        let mut s = EqSwap::new(RATE, 2, 200.0, 10_000);
        for i in 0..2000u64 {
            let x = ((i * 7919) % 200) as f32 / 100.0 - 1.0;
            let (ya, _) = s.apply((i % 2) as usize, i, x, 0.0);
            assert_eq!(ya, x);
        }
    }
}
