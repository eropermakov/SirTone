//! S-02: decode the fixture corpus with symphonia 0.6 and compare with ffmpeg (docs/09 §4, D-06).
//!
//! Usage: `cargo run --release -- <fixtures/generated>`
//! Checks per file: probe + full decode, lossless = bit-exact vs `ffmpeg -f f32le`, lossy = length
//! and SNR, accurate seek to 3.0 s lands on the right frame, broken files return errors (no panic).

use std::fs::File;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

struct Decoded {
    rate: u32,
    channels: usize,
    samples: Vec<f32>,
    decode_errors: usize,
}

fn open(
    path: &Path,
) -> Result<(Box<dyn symphonia::core::formats::FormatReader>, u32), String> {
    let file = File::open(path).map_err(|e| format!("open: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("probe: {e}"))?;
    let id = format
        .default_track(TrackType::Audio)
        .ok_or("no audio track")?
        .id;
    Ok((format, id))
}

/// Decodes the whole file, or from `seek_s` (accurate seek, frames before the target dropped).
fn decode(path: &Path, seek_s: Option<f64>) -> Result<Decoded, String> {
    let (mut format, track_id) = open(path)?;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.id == track_id)
        .ok_or("track vanished")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("not audio")?
        .clone();
    let tb = track.time_base;
    let mut decoder = codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| format!("codec: {e}"))?;

    let mut skip_until: Option<i64> = None;
    if let Some(s) = seek_s {
        let seeked = format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time { time: Time::try_from_secs_f64(s).ok_or("bad time")?, track_id: Some(track_id) },
            )
            .map_err(|e| format!("seek: {e}"))?;
        skip_until = Some(seeked.required_ts.get());
        decoder.reset();
    }

    let mut out = Decoded { rate: params.sample_rate.unwrap_or(0), channels: 0, samples: Vec::new(), decode_errors: 0 };
    let mut buf: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(format!("read: {e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio) => {
                out.rate = audio.spec().rate();
                out.channels = audio.spec().channels().count();
                buf.resize(audio.samples_interleaved(), 0.0);
                audio.copy_to_slice_interleaved(&mut buf);
                let mut start = 0usize;
                if let Some(target) = skip_until {
                    // Packet timestamps are in the track timebase; for audio it is 1/rate.
                    let pts = packet.pts.get();
                    let frames = (buf.len() / out.channels.max(1)) as i64;
                    if pts + frames <= target {
                        continue;
                    }
                    let _ = tb;
                    start = ((target - pts).max(0) as usize) * out.channels;
                    skip_until = None;
                }
                out.samples.extend_from_slice(&buf[start..]);
            }
            Err(Error::DecodeError(_)) => out.decode_errors += 1,
            Err(e) => return Err(format!("decode: {e}")),
        }
    }
    if out.samples.is_empty() {
        return Err("no audio decoded".into());
    }
    Ok(out)
}

/// Default codecs + libopus adapter (symphonia has no native Opus).
fn codecs() -> &'static symphonia::core::codecs::registry::CodecRegistry {
    static REG: std::sync::OnceLock<symphonia::core::codecs::registry::CodecRegistry> = std::sync::OnceLock::new();
    REG.get_or_init(|| {
        let mut r = symphonia::core::codecs::registry::CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut r);
        r.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
        r
    })
}

fn ffmpeg_reference(path: &Path) -> Option<Vec<f32>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "f32le", "-"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(out.stdout.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect())
}

fn snr_db(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    let (mut sig, mut noise) = (0f64, 0f64);
    for i in 0..n {
        sig += (b[i] as f64).powi(2);
        noise += (a[i] as f64 - b[i] as f64).powi(2);
    }
    if noise == 0.0 { f64::INFINITY } else { 10.0 * (sig / noise).log10() }
}

/// Finds the frame lag in −2048…+2048 that best aligns `got[g0..]` with `want[w0+lag..]` over 4096
/// frames; returns (lag, SNR dB). lag > 0: our audio is late (extra frames at the start).
fn best_lag(got: &[f32], g0: usize, want: &[f32], w0: usize, ch: usize, range: i64) -> (i64, f64) {
    let n = 4096 * ch;
    let g = &got[(g0 * ch).min(got.len())..((g0 * ch) + n).min(got.len())];
    let mut best = (0i64, f64::NEG_INFINITY);
    for lag in -range..=range {
        let o = w0 as i64 - lag;
        if o < 0 {
            continue;
        }
        let o = o as usize * ch;
        if o + g.len() > want.len() {
            continue;
        }
        let snr = snr_db(g, &want[o..o + g.len()]);
        if snr > best.1 {
            best = (lag, snr);
        }
    }
    best
}

fn max_abs_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err("PANIC".into()))
}

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "fixtures/generated".into()));
    let lossless = ["flac", "wav", "aiff", "alac"];
    let mut failures = 0;

    println!("| file | rate/ch | frames sym | frames ff | Δframes | exact/SNR | seek 3 s | speed | note |");
    println!("|---|---|---|---|---|---|---|---|---|");
    let mut files: Vec<_> = std::fs::read_dir(root.join("formats")).expect("formats dir").filter_map(|e| e.ok().map(|e| e.path())).collect();
    files.push(root.join("signals").join("gapless-a.mp3"));
    files.sort();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let is_lossless = lossless.iter().any(|k| name.starts_with(k)) || name.starts_with("mono");
        let t0 = Instant::now();
        let dec = guarded(|| decode(&path, None));
        let elapsed = t0.elapsed().as_secs_f64();
        let Ok(dec) = dec else {
            println!("| {name} | — | — | — | — | — | — | — | **{}** |", dec.err().unwrap());
            failures += 1;
            continue;
        };
        let ch = dec.channels.max(1);
        let frames = dec.samples.len() / ch;
        let speed = frames as f64 / dec.rate as f64 / elapsed;
        let reference = ffmpeg_reference(&path).unwrap_or_default();
        let ff_frames = reference.len() / ch;
        let quality = if is_lossless {
            let d = max_abs_diff(&dec.samples, &reference);
            if d == 0.0 && frames == ff_frames { "bit-exact".to_string() } else { format!("**diff {d:e}**") }
        } else {
            // Align by the best lag (encoder delay differs between decoders), report lag + SNR.
            let (lag, snr) = best_lag(&dec.samples, 0, &reference, 0, ch, 2048);
            if lag == 0 { format!("{snr:.1} dB") } else { format!("{snr:.1} dB @lag {lag:+}") }
        };
        if is_lossless && !quality.starts_with("bit") {
            failures += 1;
        }
        // Accurate seek: decoded audio after a 3.0 s seek must line up with the reference at 3.0 s.
        // Compare from 2048 frames in (MP3 bit reservoir garbles the first frame after a seek).
        let seek = match guarded(|| decode(&path, Some(3.0))) {
            Ok(s) => {
                let (lag, snr) = best_lag(&s.samples, 2048, &reference, 3 * dec.rate as usize + 2048, ch, 2048);
                if lag == 0 { format!("±0 ({snr:.0} dB)") } else { format!("**{lag:+} fr** ({snr:.0} dB)") }
            }
            Err(e) => format!("**{e}**"),
        };
        println!(
            "| {name} | {}/{} | {frames} | {ff_frames} | {} | {quality} | {seek} | {speed:.0}× | {} |",
            dec.rate,
            ch,
            frames as i64 - ff_frames as i64,
            if dec.decode_errors > 0 { format!("{} decode errors", dec.decode_errors) } else { String::new() }
        );
    }

    println!("\nbroken:");
    for entry in std::fs::read_dir(root.join("broken")).expect("broken dir").filter_map(|e| e.ok()) {
        let r = guarded(|| decode(&entry.path(), None).map(|d| d.samples.len()));
        let verdict = match &r {
            Err(e) if e == "PANIC" => {
                failures += 1;
                "PANIC (fail)".to_string()
            }
            Err(e) => format!("error: {e}"),
            Ok(n) => format!("decoded {n} samples (partial)"),
        };
        println!("- {}: {verdict}", entry.file_name().to_string_lossy());
    }

    println!("\nnames (unicode/long paths, probe only):");
    for entry in std::fs::read_dir(root.join("names")).expect("names dir").filter_map(|e| e.ok()) {
        let ok = guarded(|| open(&entry.path()).map(|_| ())).is_ok();
        if !ok {
            failures += 1;
        }
        println!("- {} ({} chars): {}", entry.file_name().to_string_lossy(), entry.path().to_string_lossy().chars().count(), if ok { "ok" } else { "FAIL" });
    }
    println!("\nfailures: {failures}");
}
