//! S-07: EfficientAT `mn10_as` (MIT; AudioSet, 527 классов) → ONNX → `tract` (чистый Rust) +
//! свой лог-мел фронтенд. Проверяем: (1) фронтенд = PyTorch `AugmentMelSTFT` (eval) на эталоне;
//! (2) логиты `tract` = PyTorch; (3) время на окно 10 с и на трек (3 окна), один поток.
//!
//! `cargo run --release -- <папка экспорта>` (mn10_as.onnx, mel_basis_128x513.f32, fixture_*.f32 —
//! их пишет `export_mn10.py`).

use std::time::Instant;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;
use tract_onnx::prelude::*;

const SR: usize = 32_000;
const N_FFT: usize = 1024;
const WIN: usize = 800;
const HOP: usize = 320;
const MELS: usize = 128;
const BINS: usize = N_FFT / 2 + 1;
const FRAMES: usize = 1000;

fn read_f32(path: &std::path::Path) -> Vec<f32> {
    std::fs::read(path)
        .expect("read")
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// `AugmentMelSTFT.forward` in eval: pre-emphasis → STFT (center, reflect) → power → mel →
/// log(x + 1e-5) → (x + 4.5) / 5. Returns [MELS][frames] row-major.
fn logmel(x: &[f32], basis: &[f32]) -> (Vec<f32>, usize) {
    // conv1d with kernel [-0.97, 1]: y[i] = x[i+1] - 0.97 x[i]
    let y: Vec<f32> = x.windows(2).map(|w| w[1] - 0.97 * w[0]).collect();
    // center=True, pad_mode reflect, n_fft/2 on each side.
    let pad = N_FFT / 2;
    let mut p = Vec::with_capacity(y.len() + 2 * pad);
    for i in (1..=pad).rev() {
        p.push(y[i]);
    }
    p.extend_from_slice(&y);
    for i in 0..pad {
        p.push(y[y.len() - 2 - i]);
    }
    let frames = 1 + (p.len() - N_FFT) / HOP;
    // Hann, periodic=False, 800 samples centred in 1024.
    let mut window = vec![0f32; N_FFT];
    let off = (N_FFT - WIN) / 2;
    for n in 0..WIN {
        window[off + n] = 0.5 - 0.5 * (std::f32::consts::TAU * n as f32 / (WIN - 1) as f32).cos();
    }
    let fft = FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
    let mut buf = vec![Complex32::new(0.0, 0.0); N_FFT];
    let mut power = vec![0f32; BINS];
    let mut out = vec![0f32; MELS * frames];
    for f in 0..frames {
        let s = f * HOP;
        for k in 0..N_FFT {
            buf[k] = Complex32::new(p[s + k] * window[k], 0.0);
        }
        fft.process(&mut buf);
        for k in 0..BINS {
            power[k] = buf[k].norm_sqr();
        }
        for m in 0..MELS {
            let row = &basis[m * BINS..(m + 1) * BINS];
            let v: f32 = row.iter().zip(&power).map(|(a, b)| a * b).sum();
            out[m * frames + f] = ((v + 1e-5).ln() + 4.5) / 5.0;
        }
    }
    (out, frames)
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

fn main() -> TractResult<()> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("export dir"));
    let basis = read_f32(&dir.join("mel_basis_128x513.f32"));
    let wave = read_f32(&dir.join("fixture_wave_32k.f32"));
    let want_mel = read_f32(&dir.join("fixture_mel.f32"));
    let want_logits = read_f32(&dir.join("fixture_logits.f32"));
    let want_emb = read_f32(&dir.join("fixture_emb.f32"));

    let t = Instant::now();
    let (mel, frames) = logmel(&wave, &basis);
    let fe_ms = t.elapsed().as_secs_f64() * 1e3;
    println!("frontend: {frames} frames, {fe_ms:.1} ms; mel max diff vs PyTorch {:.2e}", max_diff(&mel, &want_mel));

    let t = Instant::now();
    let model = tract_onnx::onnx()
        .model_for_path(dir.join("mn10_as.onnx"))?
        .with_input_fact(0, f32::fact([1, 1, MELS, FRAMES]).into())?
        .into_optimized()?
        .into_runnable()?;
    println!("model load + optimise: {:.0} ms", t.elapsed().as_secs_f64() * 1e3);

    let input: Tensor = tract_ndarray::Array4::from_shape_vec((1, 1, MELS, FRAMES), mel[..MELS * FRAMES].to_vec())?.into();
    let mut times = Vec::new();
    let mut logits = Vec::new();
    let mut emb = Vec::new();
    for _ in 0..6 {
        let t = Instant::now();
        let out = model.run(tvec!(input.clone().into()))?;
        times.push(t.elapsed().as_secs_f64() * 1e3);
        logits = out[0].to_array_view::<f32>()?.iter().copied().collect();
        emb = out[1].to_array_view::<f32>()?.iter().copied().collect();
    }
    times.sort_by(f64::total_cmp);
    println!(
        "tract: logits max diff {:.2e}, embedding max diff {:.2e}; per 10 s window median {:.0} ms (min {:.0})",
        max_diff(&logits, &want_logits),
        max_diff(&emb, &want_emb),
        times[times.len() / 2],
        times[0]
    );
    println!(
        "per track (3 windows): ≈ {:.0} ms model + {:.0} ms frontend",
        3.0 * times[times.len() / 2],
        3.0 * fe_ms
    );
    // Same top classes as PyTorch.
    let mut idx: Vec<usize> = (0..logits.len()).collect();
    idx.sort_by(|&a, &b| logits[b].total_cmp(&logits[a]));
    println!("top classes {:?}", &idx[..5]);
    Ok(())
}
