//! Analysis B2 (T3.8, `04` §10; spike S-07, D-12, D-84): an AudioSet tagger that ships inside
//! the app — EfficientAT MobileNetV3 `mn10_as` (MIT, 527 classes) as ONNX, run by `tract`
//! (pure Rust, no network, no GPU). Two 10 s windows (at 30 % and 65 % of the track, the whole
//! track when shorter) are cut from the one decode, resampled to 32 kHz (windowed sinc) and turned
//! into the model's log-mel input exactly as PyTorch `AugmentMelSTFT` does in eval (parity test:
//! 4.8e-6 on the mel, 2.2e-5 on the logits). Out: class probabilities (sigmoid, mean over
//! windows) and the 960-d embedding, projected to 64 numbers (fixed Gaussian projection,
//! Johnson–Lindenstrauss) and stored as int8 for «похожее» (T3.14).

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;
use tract_onnx::prelude::*;

pub const MODEL_RATE: u32 = 32_000;
pub const CLASSES: usize = 527;
pub const EMBEDDING: usize = 960;
pub const PROJECTED: usize = 64;
const CLIP_SAMPLES: usize = 10 * MODEL_RATE as usize;
const N_FFT: usize = 1024;
const WIN: usize = 800;
const HOP: usize = 320;
const MELS: usize = 128;
const BINS: usize = N_FFT / 2 + 1;
const FRAMES: usize = 1000;
/// Window centres as a share of the track.
const AT: [f64; 2] = [0.30, 0.65];

static MODEL: &[u8] = include_bytes!("../../../assets/models/mn10_as.onnx");
static BASIS: &[u8] = include_bytes!("../../../assets/models/mn10_as_mel_basis.f32");

/// What B2 says about a track.
#[derive(Debug, Clone, PartialEq)]
pub struct B2 {
    /// AudioSet class probabilities (mean over the windows).
    pub probs: Vec<f32>,
    /// The embedding projected to 64 numbers.
    pub embedding: [f32; PROJECTED],
    /// Power-weighted spectral centroid of the clips, Hz (feature 10, B3).
    pub centroid_hz: f32,
}

impl B2 {
    /// `[scale f32 LE][64 × i8]` for `track.embedding`.
    pub fn embedding_blob(&self) -> Vec<u8> {
        let scale = self
            .embedding
            .iter()
            .fold(0f32, |m, v| m.max(v.abs()))
            .max(1e-9)
            / 127.0;
        let mut out = scale.to_le_bytes().to_vec();
        out.extend(
            self.embedding
                .iter()
                .map(|v| (v / scale).round().clamp(-127.0, 127.0) as i8 as u8),
        );
        out
    }
}

/// The model, loaded once per analysis thread (≈ 100 ms).
pub struct Tagger {
    plan: std::sync::Arc<TypedSimplePlan>,
    basis: Vec<f32>,
    window: Vec<f32>,
    projection: Vec<f32>,
}

impl Tagger {
    pub fn builtin() -> Result<Self, String> {
        let plan = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(MODEL))
            .and_then(|m| m.with_input_fact(0, f32::fact([1, 1, MELS, FRAMES]).into()))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(|e| e.to_string())?;
        let basis: Vec<f32> = BASIS
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        if basis.len() != MELS * BINS {
            return Err("mel basis size".into());
        }
        // Hann, periodic=False, 800 samples centred in 1024 (torch.stft pads the window).
        let mut window = vec![0f32; N_FFT];
        let off = (N_FFT - WIN) / 2;
        for n in 0..WIN {
            window[off + n] =
                0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / (WIN - 1) as f32).cos();
        }
        Ok(Self {
            plan,
            basis,
            window,
            projection: projection(),
        })
    }

    /// The model's input for a 10 s clip at 32 kHz (shorter clips are zero-padded).
    pub fn logmel(&self, clip: &[f32]) -> Vec<f32> {
        self.frontend(clip).0
    }

    /// The model input and the power-weighted spectral centroid (Hz) of the clip.
    fn frontend(&self, clip: &[f32]) -> (Vec<f32>, f32) {
        let mut x = clip.to_vec();
        x.resize(CLIP_SAMPLES, 0.0);
        // Pre-emphasis as conv1d with [-0.97, 1].
        let y: Vec<f32> = x.windows(2).map(|w| w[1] - 0.97 * w[0]).collect();
        // center=True, reflect padding.
        let pad = N_FFT / 2;
        let mut p = Vec::with_capacity(y.len() + 2 * pad);
        for i in (1..=pad).rev() {
            p.push(y[i]);
        }
        p.extend_from_slice(&y);
        for i in 0..pad {
            p.push(y[y.len() - 2 - i]);
        }
        let fft = FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
        let mut buf = vec![Complex32::new(0.0, 0.0); N_FFT];
        let mut power = vec![0f32; BINS];
        let mut out = vec![0f32; MELS * FRAMES];
        let (mut num, mut den) = (0f64, 0f64);
        let hz = MODEL_RATE as f64 / N_FFT as f64;
        for f in 0..FRAMES {
            let s = f * HOP;
            for k in 0..N_FFT {
                buf[k] = Complex32::new(p[s + k] * self.window[k], 0.0);
            }
            fft.process(&mut buf);
            for (k, pw) in power.iter_mut().enumerate() {
                *pw = buf[k].norm_sqr();
                num += k as f64 * hz * f64::from(*pw);
                den += f64::from(*pw);
            }
            for m in 0..MELS {
                let row = &self.basis[m * BINS..(m + 1) * BINS];
                let v: f32 = row.iter().zip(&power).map(|(a, b)| a * b).sum();
                out[m * FRAMES + f] = ((v + 1e-5).ln() + 4.5) / 5.0;
            }
        }
        let centroid = if den > 1e-9 { (num / den) as f32 } else { 0.0 };
        (out, centroid)
    }

    /// Logits and embedding for one model input.
    pub fn run(&self, mel: Vec<f32>) -> Result<(Vec<f32>, Vec<f32>), String> {
        let input: Tensor = tract_ndarray::Array4::from_shape_vec((1, 1, MELS, FRAMES), mel)
            .map_err(|e| e.to_string())?
            .into();
        let out = self
            .plan
            .run(tvec!(input.into()))
            .map_err(|e| e.to_string())?;
        let logits = out[0]
            .to_plain_array_view::<f32>()
            .map_err(|e| e.to_string())?
            .iter()
            .copied()
            .collect();
        let emb = out[1]
            .to_plain_array_view::<f32>()
            .map_err(|e| e.to_string())?
            .iter()
            .copied()
            .collect();
        Ok((logits, emb))
    }

    /// Tags the clips (mono, any rate): probabilities and the projected embedding.
    pub fn tag(&self, clips: &[Vec<f32>], rate: u32) -> Result<B2, String> {
        if clips.is_empty() {
            return Err("no audio".into());
        }
        let mut probs = vec![0f32; CLASSES];
        let mut emb = vec![0f32; EMBEDDING];
        let mut centroid = 0f32;
        for clip in clips {
            let x = resample(clip, rate, MODEL_RATE);
            let (mel, c) = self.frontend(&x);
            centroid += c;
            let (logits, e) = self.run(mel)?;
            for (p, l) in probs.iter_mut().zip(&logits) {
                *p += 1.0 / (1.0 + (-l).exp());
            }
            for (a, b) in emb.iter_mut().zip(&e) {
                *a += b;
            }
        }
        let k = clips.len() as f32;
        for p in &mut probs {
            *p /= k;
        }
        let mut projected = [0f32; PROJECTED];
        for (i, out) in projected.iter_mut().enumerate() {
            let row = &self.projection[i * EMBEDDING..(i + 1) * EMBEDDING];
            *out = row.iter().zip(&emb).map(|(a, b)| a * b / k).sum();
        }
        Ok(B2 {
            probs,
            embedding: projected,
            centroid_hz: centroid / k,
        })
    }
}

/// A fixed Gaussian projection 960 → 64 (seeded; it never changes, or stored embeddings would
/// stop being comparable).
fn projection() -> Vec<f32> {
    let mut s = 0x5172_7469_6f6eu64;
    let mut uniform = || {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((s >> 11) as f64 / (1u64 << 53) as f64).max(1e-12)
    };
    let scale = 1.0 / (PROJECTED as f64).sqrt();
    (0..PROJECTED * EMBEDDING)
        .map(|_| {
            let (u, v) = (uniform(), uniform());
            ((-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos() * scale) as f32
        })
        .collect()
}

/// Windowed-sinc resampling (Lanczos, 16 lobes), low-passed at the lower Nyquist. Common rates
/// are rational (44.1 → 32 kHz = 320/441), so the taps are computed once per phase.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() || from == 0 || to == 0 {
        return x.to_vec();
    }
    const A: f64 = 16.0;
    const MAX_PHASES: u64 = 4096;
    let g = gcd(u64::from(from), u64::from(to));
    let (up, down) = (u64::from(to) / g, u64::from(from) / g);
    let ratio = to as f64 / from as f64;
    let cutoff = ratio.min(1.0);
    let n_out = (x.len() as u64 * up / down) as usize;
    let half = (A / cutoff).ceil() as i64;
    let taps = (2 * half) as usize;
    let sinc = |t: f64| {
        if t.abs() < 1e-9 {
            1.0
        } else {
            (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
        }
    };
    // Weight of tap k (input i0 − half + 1 + k) at fractional position `frac` of the output.
    let weight = |frac: f64, k: usize| {
        let t = (frac + (half - 1) as f64 - k as f64) * cutoff;
        if t.abs() >= A {
            0.0
        } else {
            sinc(t) * sinc(t / A)
        }
    };
    let table: Option<Vec<f64>> = (up <= MAX_PHASES).then(|| {
        (0..up)
            .flat_map(|ph| (0..taps).map(move |k| (ph, k)))
            .map(|(ph, k)| weight(ph as f64 / up as f64, k))
            .collect()
    });
    let mut w = vec![0f64; taps];
    (0..n_out)
        .map(|j| {
            let pos = j as u64 * down;
            let i0 = (pos / up) as i64;
            let ph = pos % up;
            let row: &[f64] = match &table {
                Some(t) => &t[ph as usize * taps..(ph as usize + 1) * taps],
                None => {
                    for (k, v) in w.iter_mut().enumerate() {
                        *v = weight(ph as f64 / up as f64, k);
                    }
                    &w
                }
            };
            let first = i0 - half + 1;
            let mut acc = 0.0;
            let mut wsum = 0.0;
            for (k, wk) in row.iter().enumerate() {
                let i = first + k as i64;
                if i < 0 || i as usize >= x.len() {
                    continue;
                }
                acc += wk * f64::from(x[i as usize]);
                wsum += wk;
            }
            if wsum.abs() > 1e-12 {
                (acc / wsum) as f32
            } else {
                0.0
            }
        })
        .collect()
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Cuts the analysis windows (mono) out of the decoded frames; positions need the duration
/// (from the library row); 0 — unknown: the first 10 s after 10 s.
pub struct ClipAcc {
    duration_ms: u64,
    rate: u32,
    starts: Vec<u64>,
    len: u64,
    pos: u64,
    clips: Vec<Vec<f32>>,
    /// The first 20 s — the fallback when the duration hint was wrong and no window got audio.
    head: Vec<f32>,
}

impl ClipAcc {
    pub fn new(duration_ms: u64) -> Self {
        Self {
            duration_ms,
            rate: 0,
            starts: Vec::new(),
            len: 0,
            pos: 0,
            clips: Vec::new(),
            head: Vec::new(),
        }
    }

    pub fn feed(&mut self, frames: &[f32], channels: usize, rate: u32) {
        let ch = channels.max(1);
        if self.rate == 0 {
            self.rate = rate;
            self.len = 10 * u64::from(rate);
            let total = self.duration_ms * u64::from(rate) / 1000;
            self.starts = if total == 0 {
                vec![10 * u64::from(rate)]
            } else if total <= self.len {
                vec![0]
            } else {
                AT.iter()
                    .map(|a| {
                        ((total as f64 * a) as u64)
                            .saturating_sub(self.len / 2)
                            .min(total - self.len)
                    })
                    .collect()
            };
            self.clips = self.starts.iter().map(|_| Vec::new()).collect();
        }
        for frame in frames.chunks(ch) {
            let p = self.pos;
            self.pos += 1;
            if p < 2 * self.len {
                self.head.push(frame.iter().sum::<f32>() / ch as f32);
            }
            for (k, &s) in self.starts.iter().enumerate() {
                if p >= s && p < s + self.len {
                    self.clips[k].push(frame.iter().sum::<f32>() / ch as f32);
                }
            }
        }
    }

    /// The clips that got audio, and their rate.
    pub fn finish(self) -> (Vec<Vec<f32>>, u32) {
        let min = u64::from(self.rate);
        let mut clips: Vec<Vec<f32>> = self
            .clips
            .into_iter()
            .filter(|c| c.len() as u64 >= min)
            .collect();
        if clips.is_empty() && self.head.len() as u64 >= min {
            // The last 10 s of the first 20 s.
            let from = self.head.len().saturating_sub(self.len as usize);
            clips.push(self.head[from..].to_vec());
        }
        (clips, self.rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(name: &str) -> Vec<f32> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name);
        std::fs::read(p)
            .expect("fixture")
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    }

    fn max_diff(a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0, f32::max)
    }

    /// Parity with PyTorch (`spikes/S-07-model/export_mn10.py`): the frontend and the model.
    #[test]
    fn matches_pytorch() {
        let t = Tagger::builtin().expect("model");
        let wave = read("fixture_wave_32k.f32");
        let mel = t.logmel(&wave);
        assert!(max_diff(&mel, &read("fixture_mel.f32")) < 1e-4);
        let (logits, emb) = t.run(mel).expect("run");
        assert!(max_diff(&logits, &read("fixture_logits.f32")) < 1e-3);
        assert!(max_diff(&emb, &read("fixture_emb.f32")) < 1e-3);
        // «Music» (137) is the top class of the fixture.
        let top = (0..CLASSES).max_by(|&a, &b| logits[a].total_cmp(&logits[b]));
        assert_eq!(top, Some(137));
    }

    /// The projection must never change: stored embeddings would stop being comparable.
    #[test]
    fn projection_is_pinned() {
        let p = projection();
        assert_eq!(p.len(), PROJECTED * EMBEDDING);
        let sum: f64 = p.iter().map(|v| f64::from(*v)).sum();
        let first = [p[0], p[1], p[EMBEDDING], p[p.len() - 1]];
        assert_eq!(
            format!("{first:?} {sum:.4}"),
            "[-0.10641969, -0.006897942, 0.03451837, 0.029759552] -13.2487"
        );
    }

    #[test]
    fn a_wrong_duration_falls_back_to_the_head() {
        let mut acc = ClipAcc::new(600_000); // claims 10 min, has 30 s
        let frames = vec![0.1f32; 30_000];
        acc.feed(&frames, 1, 1_000);
        let (clips, _) = acc.finish();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].len(), 10_000);
    }

    #[test]
    fn resample_keeps_a_tone() {
        let from = 44_100u32;
        let x: Vec<f32> = (0..from)
            .map(|i| (std::f32::consts::TAU * 1000.0 * i as f32 / from as f32).sin())
            .collect();
        let y = resample(&x, from, MODEL_RATE);
        assert_eq!(y.len(), MODEL_RATE as usize);
        // Compare to the ideal 1 kHz tone in the middle.
        let err = (1000..2000)
            .map(|i| {
                let ideal = (std::f32::consts::TAU * 1000.0 * i as f32 / MODEL_RATE as f32).sin();
                (y[i] - ideal).abs()
            })
            .fold(0f32, f32::max);
        assert!(err < 0.02, "{err}");
    }

    #[test]
    fn clips_at_30_and_65_percent() {
        let rate = 1_000u32;
        let mut acc = ClipAcc::new(100_000); // 100 s
        let frames: Vec<f32> = (0..100_000).map(|i| i as f32).collect();
        acc.feed(&frames, 1, rate);
        let (clips, r) = acc.finish();
        assert_eq!(r, rate);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0][0], 25_000.0);
        assert_eq!(clips[1][0], 60_000.0);
        assert_eq!(clips[0].len(), 10_000);
    }
}
