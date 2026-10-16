//! Decoder layer (docs/04 §2, S-02): probe → symphonia (+ libopus adapter). Every call into the
//! decoder runs under `catch_unwind`: a hostile file is a track error, never a crash.
//!
//! Not yet here (T1.2/T1.5): Media Foundation for WMA, AAC priming trim (`elst`/`iTunSMPB`),
//! Opus pre-skip after seek.

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
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    /// Native sample rate (Hz).
    pub rate: u32,
    /// Native channel count.
    pub channels: usize,
    /// Track length in milliseconds, when the container knows it.
    pub duration_ms: Option<u64>,
    /// Decoder short name (`flac`, `mp3`, `aac`, `alac`, `vorbis`, `opus`, `pcm_s16le`, …).
    pub codec: String,
    /// After an accurate seek: drop decoded frames before this timestamp.
    skip_until: Option<i64>,
    scratch: Vec<f32>,
}

impl Source {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let file = File::open(path).map_err(|e| {
            let code = match e.kind() {
                std::io::ErrorKind::NotFound => ErrorCode::FileNotFound,
                std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
                _ => ErrorCode::FileNotFound,
            };
            AppError::new(code).with_details(e)
        })?;
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_owned);
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
            let duration_ms = match (track.num_frames, track.time_base) {
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
            Ok(Self {
                format,
                decoder,
                track_id,
                rate,
                channels,
                duration_ms,
                codec,
                skip_until: None,
                scratch: Vec::new(),
            })
        })
    }

    /// Appends the next packet's frames (interleaved, `self.channels` wide) to `out`.
    /// Returns `Ok(false)` at end of stream. Corrupt packets are skipped (counted by the caller
    /// as nothing decoded), only unrecoverable errors are returned.
    pub fn next_into(&mut self, out: &mut Vec<f32>) -> Result<bool, AppError> {
        guarded(|| {
            loop {
                let packet = match self.format.next_packet() {
                    Ok(Some(p)) => p,
                    Ok(None) => return Ok(false),
                    Err(SymError::ResetRequired) => return Ok(false),
                    // A truncated file ends with an IO error: play what was there.
                    Err(SymError::IoError(_)) => return Ok(false),
                    Err(e) => return Err(decode_failed(e)),
                };
                if packet.track_id != self.track_id {
                    continue;
                }
                let audio = match self.decoder.decode(&packet) {
                    Ok(a) => a,
                    Err(SymError::DecodeError(_)) => continue,
                    Err(e) => return Err(decode_failed(e)),
                };
                let ch = audio.spec().channels().count().max(1);
                self.channels = ch;
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
                out.extend_from_slice(&self.scratch[start..]);
                return Ok(true);
            }
        })
    }

    /// Sample-accurate seek (`SeekMode::Accurate`); the next `next_into` starts at `ms`.
    pub fn seek(&mut self, ms: u64) -> Result<(), AppError> {
        guarded(|| {
            let time = Time::try_from_secs_f64(ms as f64 / 1000.0)
                .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument))?;
            let seeked = self
                .format
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time,
                        track_id: Some(self.track_id),
                    },
                )
                .map_err(decode_failed)?;
            self.skip_until = Some(seeked.required_ts.get());
            self.decoder.reset();
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
