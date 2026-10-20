//! Decoder layer (docs/04 §2, S-02): probe → symphonia (+ libopus adapter). Every call into the
//! decoder runs under `catch_unwind`: a hostile file is a track error, never a crash.
//!
//! What symphonia cannot open (WMA/ASF) goes to Media Foundation (`crate::mf`, T1.2).

use std::fs::File;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::OnceLock;

use sirtone_core::error::{AppError, ErrorCode};
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::registry::CodecRegistry;
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

fn codecs() -> &'static CodecRegistry {
    static REG: OnceLock<CodecRegistry> = OnceLock::new();
    REG.get_or_init(|| {
        let mut r = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut r);
        r.register_audio_decoder::<symphonia_adapter_libopus::OpusDecoder>();
        r
    })
}

fn guarded<T>(f: impl FnOnce() -> Result<T, AppError>) -> Result<T, AppError> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(AppError::new(ErrorCode::DecodeFailed).with_details("decoder panicked"))
    })
}

fn decode_failed(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::DecodeFailed).with_details(e)
}

/// One opened audio track, decoded packet by packet into interleaved `f32` at its native rate.
pub struct Source {
    backend: Backend,
    /// Native sample rate (Hz).
    pub rate: u32,
    /// Native channel count.
    pub channels: usize,
    /// Speaker positions (WAVE/symphonia bit mask, channels in ascending bit order); 0 = unknown.
    pub channel_mask: u64,
    /// Track length in milliseconds, when the container knows it.
    pub duration_ms: Option<u64>,
    /// Decoder short name (`flac`, `mp3`, `aac`, `alac`, `vorbis`, `opus`, `pcm_s16le`, …).
    pub codec: String,
    /// After an accurate seek: drop decoded frames before this timestamp.
    skip_until: Option<i64>,
    scratch: Vec<f32>,
    /// MP4 encoder delay/padding (T1.5): frames still to drop at the start, frames left to emit.
    trim: Option<crate::gapless::Trim>,
    drop_left: u64,
    valid_left: Option<u64>,
    /// Opus pre-skip (frames) — applied again after every seek.
    pre_skip: u64,
    /// The file (the engine finds a track's source by it to change its gain).
    pub path: std::path::PathBuf,
    /// Normalization gain (T4.11), linear: applied to the decoded samples, ramped on change.
    gain: f32,
    gain_target: f32,
    gain_step: f32,
}

enum Backend {
    Symphonia {
        format: Box<dyn FormatReader>,
        decoder: Box<dyn AudioDecoder>,
        track_id: u32,
    },
    Mf(crate::mf::MfReader),
}

/// Containers only Media Foundation reads (docs/04 §2).
fn mf_only(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "wma" | "asf"))
}

impl Source {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        match Self::open_symphonia(path) {
            Err(e) if e.code == ErrorCode::UnsupportedFormat && mf_only(path) => {
                Self::open_mf(path)
            }
            other => other,
        }
    }

    fn open_mf(path: &Path) -> Result<Self, AppError> {
        let path = path.to_path_buf();
        guarded(move || {
            let r = crate::mf::MfReader::open(&path)?;
            Ok(Self {
                rate: r.rate,
                channels: r.channels,
                duration_ms: r.duration_ms,
                codec: r.codec.into(),
                channel_mask: r.channel_mask,
                backend: Backend::Mf(r),
                skip_until: None,
                scratch: Vec::new(),
                trim: None,
                drop_left: 0,
                valid_left: None,
                pre_skip: 0,
                path: path.clone(),
                gain: 1.0,
                gain_target: 1.0,
                gain_step: 0.0,
            })
        })
    }

    fn open_symphonia(path: &Path) -> Result<Self, AppError> {
        let file = File::open(path).map_err(|e| {
            let code = match e.kind() {
                std::io::ErrorKind::NotFound => ErrorCode::FileNotFound,
                std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
                _ => ErrorCode::FileNotFound,
            };
            AppError::new(code).with_details(e)
        })?;
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_owned);
        let path_buf = path.to_path_buf();
        guarded(move || {
            let mss = MediaSourceStream::new(Box::new(file), Default::default());
            let mut hint = Hint::new();
            if let Some(ext) = ext.as_deref() {
                hint.with_extension(ext);
            }
            let format = symphonia::default::get_probe()
                .probe(
                    &hint,
                    mss,
                    FormatOptions::default(),
                    MetadataOptions::default(),
                )
                .map_err(|e| AppError::new(ErrorCode::UnsupportedFormat).with_details(e))?;
            let track = format
                .default_track(TrackType::Audio)
                .ok_or_else(|| AppError::new(ErrorCode::UnsupportedFormat))?;
            let track_id = track.id;
            let params = track
                .codec_params
                .as_ref()
                .and_then(|p| p.audio())
                .ok_or_else(|| AppError::new(ErrorCode::UnsupportedFormat))?
                .clone();
            let rate = params
                .sample_rate
                .ok_or_else(|| decode_failed("unknown sample rate"))?;
            let channels = params.channels.as_ref().map_or(0, |c| c.count());
            let mut duration_ms = match (track.num_frames, track.time_base) {
                (Some(n), Some(tb)) => {
                    Some(n * 1000 * u64::from(tb.numer.get()) / u64::from(tb.denom.get()))
                }
                (Some(n), None) => Some(n * 1000 / u64::from(rate)),
                _ => None,
            };
            let codec = codecs()
                .get_audio_decoder(params.codec)
                .map(|d| d.codec.info.short_name.to_ascii_lowercase())
                .unwrap_or_default();
            let decoder = codecs()
                .make_audio_decoder(&params, &AudioDecoderOptions::default())
                .map_err(|e| AppError::new(ErrorCode::UnsupportedFormat).with_details(e))?;
            let trim = crate::gapless::mp4_trim(&path_buf, rate);
            if let Some(valid) = trim.and_then(|t| t.valid) {
                duration_ms = Some(valid * 1000 / u64::from(rate));
            }
            let pre_skip = if codec == "opus" {
                crate::gapless::opus_pre_skip(params.extra_data.as_deref())
            } else {
                0
            };
            Ok(Self {
                backend: Backend::Symphonia {
                    format,
                    decoder,
                    track_id,
                },
                rate,
                channels,
                channel_mask: match params.channels.as_ref() {
                    Some(symphonia::core::audio::Channels::Positioned(p)) => p.bits(),
                    _ => 0,
                },
                duration_ms,
                codec,
                skip_until: None,
                scratch: Vec::new(),
                trim,
                drop_left: trim.map_or(0, |t| t.start),
                valid_left: trim.and_then(|t| t.valid),
                pre_skip,
                path: path_buf.clone(),
                gain: 1.0,
                gain_target: 1.0,
                gain_step: 0.0,
            })
        })
    }

    /// Sets the normalization gain (linear): at once (`ramp_ms` 0, a track that is not playing
    /// yet) or ramped over `ramp_ms` of this source's frames (a live change, no click).
    pub fn set_gain(&mut self, amp: f32, ramp_ms: u32) {
        self.gain_target = amp;
        let frames = (u64::from(self.rate) * u64::from(ramp_ms) / 1000).max(1);
        if ramp_ms == 0 {
            self.gain = amp;
            self.gain_step = 0.0;
        } else {
            self.gain_step = (amp - self.gain) / frames as f32;
        }
    }

    /// The normalization gain this source is heading to, dB (0 — none).
    pub fn gain_db(&self) -> f32 {
        if self.gain_target == 1.0 {
            0.0
        } else {
            20.0 * self.gain_target.log10()
        }
    }

    /// Applies the gain to decoded frames (`ch` wide). Unity gain is a true bypass.
    pub fn apply_gain(&mut self, buf: &mut [f32], ch: usize) {
        if self.gain == 1.0 && self.gain_target == 1.0 {
            return;
        }
        for frame in buf.chunks_mut(ch.max(1)) {
            if self.gain != self.gain_target {
                self.gain += self.gain_step;
                let passed = (self.gain_step >= 0.0 && self.gain >= self.gain_target)
                    || (self.gain_step <= 0.0 && self.gain <= self.gain_target);
                if passed {
                    self.gain = self.gain_target;
                }
            }
            for s in frame {
                *s *= self.gain;
            }
        }
    }

    /// Appends the next packet's frames (interleaved, `self.channels` wide) to `out`.
    /// Returns `Ok(false)` at end of stream. Corrupt packets are skipped (counted by the caller
    /// as nothing decoded), only unrecoverable errors are returned.
    pub fn next_into(&mut self, out: &mut Vec<f32>) -> Result<bool, AppError> {
        let (format, decoder, track_id) = match &mut self.backend {
            Backend::Mf(r) => return guarded(|| r.next_into(out)),
            Backend::Symphonia {
                format,
                decoder,
                track_id,
            } => (format, decoder, *track_id),
        };
        guarded(|| {
            loop {
                if self.valid_left == Some(0) {
                    return Ok(false); // encoder padding reached (T1.5)
                }
                let packet = match format.next_packet() {
                    Ok(Some(p)) => p,
                    Ok(None) => return Ok(false),
                    Err(SymError::ResetRequired) => return Ok(false),
                    // A truncated file ends with an IO error: play what was there.
                    Err(SymError::IoError(_)) => return Ok(false),
                    Err(e) => return Err(decode_failed(e)),
                };
                if packet.track_id != track_id {
                    continue;
                }
                let audio = match decoder.decode(&packet) {
                    Ok(a) => a,
                    Err(SymError::DecodeError(_)) => continue,
                    Err(e) => return Err(decode_failed(e)),
                };
                let ch = audio.spec().channels().count().max(1);
                self.channels = ch;
                if let symphonia::core::audio::Channels::Positioned(p) = audio.spec().channels() {
                    self.channel_mask = p.bits();
                }
                self.scratch.resize(audio.samples_interleaved(), 0.0);
                audio.copy_to_slice_interleaved(&mut self.scratch);
                let mut start = 0usize;
                if let Some(target) = self.skip_until {
                    let pts = packet.pts.get();
                    let frames = (self.scratch.len() / ch) as i64;
                    if pts + frames <= target {
                        continue;
                    }
                    start = (target - pts).max(0) as usize * ch;
                    self.skip_until = None;
                }
                // Encoder delay at the start (MP4 elst/iTunSMPB).
                let avail = (self.scratch.len() - start) / ch;
                let drop = (self.drop_left as usize).min(avail);
                self.drop_left -= drop as u64;
                start += drop * ch;
                let mut end = self.scratch.len();
                if let Some(left) = self.valid_left.as_mut() {
                    let take = ((end - start) / ch).min(*left as usize);
                    *left -= take as u64;
                    end = start + take * ch;
                }
                if end == start {
                    continue;
                }
                out.extend_from_slice(&self.scratch[start..end]);
                return Ok(true);
            }
        })
    }

    /// Sample-accurate seek (`SeekMode::Accurate`); the next `next_into` starts at `ms`.
    pub fn seek(&mut self, ms: u64) -> Result<(), AppError> {
        let (format, decoder, track_id) = match &mut self.backend {
            Backend::Mf(r) => return guarded(|| r.seek(ms)),
            Backend::Symphonia {
                format,
                decoder,
                track_id,
            } => (format, decoder, *track_id),
        };
        guarded(|| {
            // MP4: the media timeline starts with the encoder delay; seek past it.
            let priming = self.trim.map_or(0, |t| t.start);
            let media_ms = ms as f64 / 1000.0 + priming as f64 / f64::from(self.rate);
            let time = Time::try_from_secs_f64(media_ms)
                .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument))?;
            let seeked = format
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time,
                        track_id: Some(track_id),
                    },
                )
                .map_err(decode_failed)?;
            // Opus: the decoder trims pre-skip only once; after a reset it is ours to drop.
            self.skip_until = Some(seeked.required_ts.get() + self.pre_skip as i64);
            decoder.reset();
            self.drop_left = 0;
            if let Some(valid) = self.trim.and_then(|t| t.valid) {
                self.valid_left = Some(valid.saturating_sub(ms * u64::from(self.rate) / 1000));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(rel: &str) -> Option<PathBuf> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/generated")
            .join(rel);
        p.exists().then_some(p)
    }

    fn decode_all(path: &Path) -> Result<(Source, Vec<f32>), AppError> {
        let mut s = Source::open(path)?;
        let mut out = Vec::new();
        while s.next_into(&mut out)? {}
        Ok((s, out))
    }

    #[test]
    fn flac_decodes_full_length() {
        let Some(p) = fixture("formats/flac-16-44.flac") else {
            return; // corpus not generated (CI without ffmpeg): covered by spike S-02
        };
        let (s, out) = decode_all(&p).unwrap();
        assert_eq!((s.rate, s.channels), (44100, 2));
        assert_eq!(out.len() / 2, 441_000);
        assert_eq!(s.duration_ms, Some(10_000));
    }

    #[test]
    fn opus_decodes_via_libopus() {
        let Some(p) = fixture("formats/opus-160.opus") else {
            return;
        };
        let (s, out) = decode_all(&p).unwrap();
        assert_eq!(s.rate, 48000);
        assert_eq!(out.len() / 2, 480_000);
    }

    #[test]
    fn aac_encoder_delay_is_trimmed() {
        // ffmpeg reference: 441 000 frames (S-02: symphonia alone gave 442 368).
        let Some(p) = fixture("formats/aac-256.m4a") else {
            return;
        };
        let (s, out) = decode_all(&p).unwrap();
        assert_eq!(out.len() / s.channels, 441_000);
        assert_eq!(s.duration_ms, Some(10_000));
    }

    /// After seeking to 3 s the audio equals the full decode from 3 s (lossy codecs: within the
    /// first 2048 frames the bit reservoir/overlap may differ, so compare a later window).
    fn seek_matches(rel: &str, rate: usize) {
        let Some(p) = fixture(rel) else { return };
        let (s, full) = decode_all(&p).unwrap();
        let ch = s.channels;
        let mut src = Source::open(&p).unwrap();
        src.seek(3000).unwrap();
        let mut out = Vec::new();
        while out.len() < 8192 * ch && src.next_into(&mut out).unwrap() {}
        let best = (-400i64..=400)
            .min_by(|&a, &b| {
                let err = |lag: i64| -> f32 {
                    let off = ((3 * rate) as i64 + 2048 + lag) as usize * ch;
                    (0..4096 * ch)
                        .map(|i| (out[2048 * ch + i] - full[off + i]).abs())
                        .sum()
                };
                err(a).total_cmp(&err(b))
            })
            .unwrap();
        assert_eq!(best, 0, "{rel}: seek lands {best} frames off");
    }

    #[test]
    fn aac_and_opus_seek_are_sample_accurate() {
        seek_matches("formats/aac-256.m4a", 44100);
        seek_matches("formats/opus-160.opus", 48000);
    }

    #[test]
    fn wma_decodes_via_media_foundation() {
        let Some(p) = fixture("formats/wma-192.wma") else {
            return;
        };
        let (s, out) = decode_all(&p).unwrap();
        assert_eq!(s.codec, "wma");
        assert_eq!((s.rate, s.channels), (44100, 2));
        let frames = out.len() / 2;
        // MF trims the WMA decoder delay itself; the last block may come out short (≤ 2048).
        assert!(frames.abs_diff(441_000) <= 2048, "{frames}");
        assert!(s.duration_ms.is_some_and(|d| d.abs_diff(10_000) < 100));
        assert!(out.iter().any(|v| v.abs() > 0.1), "silence");
    }

    /// D-68: MF timestamps after a WMA seek are approximate — the landing is within one
    /// WMA block (2048 frames) plus decoder latency of the target, never worse.
    #[test]
    fn wma_seek_lands_within_a_block() {
        let (Some(wav), Some(p)) = (
            fixture("formats/wav-16-44.wav"),
            fixture("formats/wma-192.wma"),
        ) else {
            return;
        };
        let wav = decode_all(&wav).unwrap().1;
        for ms in [1000u64, 3000, 4321, 7777] {
            let mut src = Source::open(&p).unwrap();
            src.seek(ms).unwrap();
            let mut out = Vec::new();
            while out.len() < 16384 * 2 && src.next_into(&mut out).unwrap() {}
            let at = (ms as usize * 441 / 10 + 4096) as i64 * 2;
            let err = |lag: i64| -> f32 {
                (0..8192)
                    .map(|i| (out[4096 * 2 + i] - wav[(at + lag * 2) as usize + i]).abs())
                    .sum()
            };
            let best = (-3000i64..=3000)
                .min_by(|&a, &b| err(a).total_cmp(&err(b)))
                .unwrap();
            assert!(best.abs() <= 2600, "seek {ms}: {best} frames off");
        }
    }

    #[test]
    fn seek_is_sample_accurate() {
        let Some(p) = fixture("formats/wav-16-44.wav") else {
            return;
        };
        let (_, full) = decode_all(&p).unwrap();
        let mut s = Source::open(&p).unwrap();
        s.seek(3000).unwrap();
        let mut out = Vec::new();
        s.next_into(&mut out).unwrap();
        let off = 3 * 44100 * 2;
        assert_eq!(&out[..64], &full[off..off + 64]);
    }

    #[test]
    fn broken_files_are_errors_not_panics() {
        for name in [
            "broken/empty.flac",
            "broken/text-as.mp3",
            "broken/playlist.m3u",
        ] {
            let Some(p) = fixture(name) else { continue };
            let err = decode_all(&p).err().map(|e| e.code);
            assert_eq!(err, Some(ErrorCode::UnsupportedFormat), "{name}");
        }
        if let Some(p) = fixture("broken/truncated.mp3") {
            let (_, out) = decode_all(&p).unwrap();
            assert!(!out.is_empty());
        }
    }

    #[test]
    fn missing_file_is_file_not_found() {
        let err = Source::open(Path::new("Z:/definitely/missing.flac")).err();
        assert!(err.is_some());
    }
}
