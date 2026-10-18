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
    let shared = Shared::new();
    let (mut prod, mut cons) = rtrb::RingBuffer::<f32>::new(fmt.rate as usize * fmt.channels);
    let mut out: Vec<f32> = Vec::new();
    let mut rest = files.iter();
    let Some(first) = rest.next() else {
        return Ok(out);
    };
    let mut deck = new_deck(Source::open(first)?, first.clone(), fmt, 0)?;
    let mut next = rest.next();
    loop {
        // Same-format next track joins the chain before the end (engine::pump does the same).
        if deck.chained.is_none()
            && let Some(p) = next
        {
            let src = Source::open(p)?;
            if src.rate == deck.source.rate && src.channels == deck.source.channels {
                deck.chained = Some((src, p.clone()));
                next = rest.next();
            }
        }
        fill(&mut deck, fmt, &mut prod, &shared, true)?;
        if let Ok(chunk) = cons.read_chunk(cons.slots()) {
            let (a, b) = chunk.as_slices();
            out.extend_from_slice(a);
            out.extend_from_slice(b);
            chunk.commit_all();
        }
        if deck.eof && deck.chained.is_none() {
            match next {
                // Other format: a fresh deck continues right after the last frame.
                Some(p) => {
                    deck = new_deck(Source::open(p)?, p.clone(), fmt, 0)?;
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
