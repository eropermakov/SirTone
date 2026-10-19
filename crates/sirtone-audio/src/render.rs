//! Offline render (T1.7, NFR-AUD-001, docs/08 §2): the playback chain — decoder, channel map,
//! resampler, gapless joints, cubic volume and the start ramp of the render thread — run without a
//! device into a buffer. Same code as live playback (`engine::fill`), so «golden» outputs catch
//! any change in what the listener would hear.

use std::path::{Path, PathBuf};

use sirtone_core::error::AppError;

use crate::decode::Source;
use crate::engine::{fill, new_deck};
use crate::output::{DeviceFormat, PAUSE_RAMP_S, Shared, VOLUME_SMOOTH_S};

/// Renders `files` back to back (gapless, like a queue) at `fmt`, player volume `volume` (0..1).
/// Returns interleaved `f32` samples.
pub fn render(files: &[PathBuf], fmt: DeviceFormat, volume: f32) -> Result<Vec<f32>, AppError> {
    let with: Vec<(PathBuf, f32)> = files.iter().map(|f| (f.clone(), 0.0)).collect();
    render_with_gains(&with, fmt, volume)
}

/// Opens a track with its normalization gain (dB), like the engine does.
fn open(path: &Path, db: f32) -> Result<Source, AppError> {
    let mut s = Source::open(path)?;
    s.set_gain(sirtone_core::loudness::amplitude(db), 0);
    Ok(s)
}

/// [`render`] with a normalization gain (dB) per file (T4.11).
pub fn render_with_gains(
    files: &[(PathBuf, f32)],
    fmt: DeviceFormat,
    volume: f32,
) -> Result<Vec<f32>, AppError> {
    let shared = Shared::new();
    let (mut prod, mut cons) = rtrb::RingBuffer::<f32>::new(fmt.rate as usize * fmt.channels);
    let mut out: Vec<f32> = Vec::new();
    let mut rest = files.iter();
    let Some((first, g)) = rest.next() else {
        return Ok(out);
    };
    let mut deck = new_deck(open(first, *g)?, first.clone(), fmt, 0)?;
    let mut next = rest.next();
    loop {
        // Same-format next track joins the chain before the end (engine::pump does the same).
        if deck.chained.is_none()
            && let Some((p, g)) = next
        {
            let src = open(p, *g)?;
            if src.rate == deck.source.rate && src.channels == deck.source.channels {
                deck.chained = Some((src, p.clone()));
                next = rest.next();
            }
        }
        fill(&mut deck, fmt, &mut prod, &shared, true, None)?;
        if let Ok(chunk) = cons.read_chunk(cons.slots()) {
            let (a, b) = chunk.as_slices();
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            chunk.commit_all();
        }
        if deck.eof && deck.chained.is_none() {
            match next {
                // Other format: a fresh deck continues right after the last frame.
                Some((p, g)) => {
                    deck = new_deck(open(p, *g)?, p.clone(), fmt, 0)?;
                    next = rest.next();
                }
                None => break,
            }
        }
    }
    // The render thread's per-frame gain: cubic volume, 8 ms smoothing, 15 ms start ramp.
    let rate = fmt.rate as f32;
    let ramp_step = 1.0 / (PAUSE_RAMP_S * rate);
    let smooth = 1.0 - (-1.0 / (VOLUME_SMOOTH_S * rate)).exp();
    let target = volume.clamp(0.0, 1.0).powi(3);
    let (mut gain, mut ramp) = (1.0f32, 0.0f32);
    for frame in out.chunks_exact_mut(fmt.channels.max(1)) {
        ramp = (ramp + ramp_step).min(1.0);
        gain += (target - gain) * smooth;
        for s in frame {
            *s *= gain * ramp;
        }
    }
    Ok(out)
}

/// 32-bit float WAV (`WAVE_FORMAT_IEEE_FLOAT`), little-endian.
pub fn wav_bytes(samples: &[f32], fmt: DeviceFormat) -> Vec<u8> {
    let ch = fmt.channels as u16;
    let data_len = (samples.len() * 4) as u32;
    let mut b = Vec::with_capacity(44 + samples.len() * 4);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    b.extend_from_slice(&ch.to_le_bytes());
    b.extend_from_slice(&fmt.rate.to_le_bytes());
    b.extend_from_slice(&(fmt.rate * u32::from(ch) * 4).to_le_bytes());
    b.extend_from_slice(&(ch * 4).to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&s.to_le_bytes());
    }
    b
}

/// Renders straight to a WAV file.
pub fn render_to_wav(
    files: &[PathBuf],
    fmt: DeviceFormat,
    volume: f32,
    out: &Path,
) -> Result<usize, AppError> {
    let samples = render(files, fmt, volume)?;
    std::fs::write(out, wav_bytes(&samples, fmt)).map_err(|e| {
        AppError::new(sirtone_core::error::ErrorCode::PermissionDenied).with_details(e)
    })?;
    Ok(samples.len() / fmt.channels.max(1))
}

/// A then B through a planned handover (T4.9: the engine's own overlap code, no device), at
/// `fmt`, without the render thread's volume. For the automix metrics (`04` §4.5).
pub fn render_mix(
    a: &Path,
    b: &Path,
    m: crate::engine::MixSpec,
    fmt: DeviceFormat,
) -> Result<Vec<f32>, AppError> {
    use crate::engine::{Mixing, mix_fill};
    let shared = Shared::new();
    let (mut prod, mut cons) = rtrb::RingBuffer::<f32>::new(fmt.rate as usize * fmt.channels);
    let mut out: Vec<f32> = Vec::new();
    let drain = |cons: &mut rtrb::Consumer<f32>, out: &mut Vec<f32>| {
        if let Ok(chunk) = cons.read_chunk(cons.slots()) {
            let (x, y) = chunk.as_slices();
            out.extend_from_slice(x);
            out.extend_from_slice(y);
            chunk.commit_all();
        }
    };
    let rate = u64::from(fmt.rate);
    let mut da = new_deck(open(a, 0.0)?, a.to_path_buf(), fmt, 0)?;
    da.stop_frame = Some(m.a_out_ms * rate / 1000);
    while !da.stopped {
        fill(&mut da, fmt, &mut prod, &shared, true, None)?;
        drain(&mut cons, &mut out);
    }
    let at = da.pushed - (da.ready.len() / fmt.channels.max(1)) as u64;
    let stop = (m.a_stop_ms * rate / 1000).max(at);
    da.stop_frame = Some(stop);
    da.stopped = false;
    let mut src = open(b, 0.0)?;
    if m.b_start_ms > 0 {
        src.seek(m.b_start_ms)?;
    }
    let mut db = new_deck(src, b.to_path_buf(), fmt, m.b_start_ms * rate / 1000)?;
    let mut mix = Mixing::new(da, stop - at, m.a_fade, m.b_fade);
    while !mix_fill(&mut mix, &mut db, fmt, &mut prod, &shared, true, None)? {
        drain(&mut cons, &mut out);
    }
    drain(&mut cons, &mut out);
    while !(db.ready.is_empty() && db.eof) {
        fill(&mut db, fmt, &mut prod, &shared, true, None)?;
        drain(&mut cons, &mut out);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// T4.11: a gapless pair with −6.02 dB and 0 dB switches gain exactly at the joint.
    #[test]
    fn normalization_gain_switches_at_the_gapless_joint() {
        let fmt = DeviceFormat {
            rate: 48_000,
            channels: 2,
        };
        let dir = std::env::temp_dir().join(format!("sirtone-norm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A steady 0.5 level (square-ish: ±0.5) one second each.
        let tone: Vec<f32> = (0..48_000 * 2)
            .map(|i| if (i / 2 / 24) % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        std::fs::write(&a, wav_bytes(&tone, fmt)).unwrap();
        std::fs::write(&b, wav_bytes(&tone, fmt)).unwrap();
        let out = render_with_gains(&[(a, -6.0206), (b, 0.0)], fmt, 1.0).unwrap();
        let peak = |from: usize, to: usize| {
            out[from * 2..to * 2]
                .iter()
                .fold(0f32, |m, s| m.max(s.abs()))
        };
        // After the start ramp: 0.25 in the first track, 0.5 from the first frame of the second.
        assert!(
            (peak(2_000, 47_990) - 0.25).abs() < 1e-3,
            "{}",
            peak(2_000, 47_990)
        );
        assert!(
            (peak(48_000, 48_010) - 0.5).abs() < 1e-3,
            "{}",
            peak(48_000, 48_010)
        );
        assert!((peak(47_990, 48_000) - 0.25).abs() < 1e-3);
        // 0 dB is a true bypass: the second track is bit-identical to its file.
        assert_eq!(&out[48_100 * 2..48_200 * 2], &tone[100 * 2..200 * 2]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A live change ramps (no click): over 30 ms from 1.0 to 0.5.
    #[test]
    fn a_gain_change_ramps() {
        let dir = std::env::temp_dir().join(format!("sirtone-ramp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fmt = DeviceFormat {
            rate: 48_000,
            channels: 1,
        };
        let p = dir.join("c.wav");
        std::fs::write(&p, wav_bytes(&vec![1.0f32; 48_000], fmt)).unwrap();
        let mut s = Source::open(&p).unwrap();
        s.set_gain(0.5, 30);
        let mut buf = vec![1.0f32; 2_000];
        s.apply_gain(&mut buf, 1);
        assert!(buf[0] > 0.99 && buf[0] < 1.0);
        assert!((buf[720] - 0.75).abs() < 0.01, "{}", buf[720]);
        assert!((buf[1_500] - 0.5).abs() < 1e-6);
        assert!(buf.windows(2).all(|w| w[1] <= w[0]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sine_file(dir: &Path, name: &str, freq: f32, secs: f32, lead: f32, tail: f32) -> PathBuf {
        let fmt = DeviceFormat {
            rate: 48_000,
            channels: 2,
        };
        let n = (48_000.0 * secs) as usize;
        let (l, t) = ((48_000.0 * lead) as usize, (48_000.0 * tail) as usize);
        let mut x = Vec::with_capacity(n * 2);
        for i in 0..n {
            let v = if i < l || i >= n - t {
                0.0
            } else {
                0.5 * (std::f32::consts::TAU * freq * i as f32 / 48_000.0).sin()
            };
            x.push(v);
            x.push(v);
        }
        let p = dir.join(name);
        std::fs::write(&p, wav_bytes(&x, fmt)).unwrap();
        p
    }

    fn rms_db(x: &[f32]) -> f64 {
        let e = x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len().max(1) as f64;
        10.0 * e.log10()
    }

    /// T4.9 metrics (`04` §4.5): an equal-power crossfade of two uncorrelated tones keeps the
    /// level (≤ 1 dB), has no click, and the output is A up to the overlap, the overlap, then B.
    #[test]
    fn a_crossfade_keeps_the_level_without_clicks() {
        let dir = std::env::temp_dir().join(format!("sirtone-mix-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = sine_file(&dir, "a.wav", 440.0, 10.0, 0.0, 0.0);
        let b = sine_file(&dir, "b.wav", 660.0, 10.0, 0.0, 0.0);
        let fmt = DeviceFormat {
            rate: 48_000,
            channels: 2,
        };
        let m = crate::engine::MixSpec {
            a_out_ms: 6_000,
            a_stop_ms: 10_000,
            b_start_ms: 0,
            a_fade: true,
            b_fade: true,
        };
        let out = render_mix(&a, &b, m, fmt).unwrap();
        // Length: 6 s of A, 4 s overlap, 6 s of B.
        assert_eq!(out.len() / 2, 16 * 48_000);
        let at = |s: f64| (s * 48_000.0) as usize * 2;
        let base = rms_db(&out[at(2.0)..at(3.0)]);
        // 400 ms around the middle of the overlap and its edges.
        for mid in [6.2, 8.0, 9.8] {
            let d = rms_db(&out[at(mid - 0.2)..at(mid + 0.2)]) - base;
            assert!(d.abs() <= 1.0, "{mid} s: {d:.2} dB");
        }
        // No click: the biggest sample step at the joints is no bigger than in the steady parts.
        let step = |x: &[f32]| {
            let l: Vec<f32> = x.as_chunks::<2>().0.iter().map(|f| f[0]).collect();
            l.windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0f32, f32::max)
        };
        let steady = step(&out[at(1.0)..at(2.0)]).max(step(&out[at(13.0)..at(14.0)]));
        for joint in [6.0, 10.0] {
            let s = step(&out[at(joint - 0.05)..at(joint + 0.05)]);
            assert!(s <= steady * 1.05, "{joint} s: {s} vs {steady}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// L1: A's tail silence and B's lead silence are cut, the joint has no gap.
    #[test]
    fn silences_are_trimmed_at_the_joint() {
        let dir = std::env::temp_dir().join(format!("sirtone-trim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = sine_file(&dir, "a.wav", 440.0, 5.0, 0.0, 1.0);
        let b = sine_file(&dir, "b.wav", 440.0, 5.0, 1.0, 0.0);
        let fmt = DeviceFormat {
            rate: 48_000,
            channels: 2,
        };
        let m = crate::engine::MixSpec {
            a_out_ms: 4_000,
            a_stop_ms: 4_000,
            b_start_ms: 1_000,
            a_fade: false,
            b_fade: false,
        };
        let out = render_mix(&a, &b, m, fmt).unwrap();
        assert_eq!(out.len() / 2, 8 * 48_000);
        // No silent 10 ms block around the joint (4 s).
        let blocks = out[(3 * 48_000) * 2..(5 * 48_000) * 2].chunks(960);
        assert!(
            blocks
                .into_iter()
                .all(|bl| bl.iter().any(|v| v.abs() > 0.01))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
