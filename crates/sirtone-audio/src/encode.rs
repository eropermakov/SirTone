//! Economy copies (D-146, M8.1): any track the core can decode → an Ogg Opus file at 128 or
//! 64 kbit/s, stereo, 48 kHz — what Plexamp does for downloads (`01d` [M49]). The original is
//! only read. The copy is written to `<dst>.part` and renamed at the end, so a cut never
//! leaves a broken copy under the real name.
//!
//! Opus (RFC 6716) in Ogg (RFC 7845): `OpusHead`, `OpusTags`, then 20 ms packets; granule
//! positions count 48 kHz samples including the encoder's pre-skip.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use opusic_sys as op;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use sirtone_core::error::{AppError, ErrorCode};

use crate::decode::Source;

const RATE: u32 = 48_000;
/// 20 ms at 48 kHz — the usual Opus frame for music.
const FRAME: usize = 960;
const CHUNK: usize = 1024;
const MAX_PACKET: usize = 4000;
const OPUS_GET_LOOKAHEAD_REQUEST: i32 = 4027;

/// The two economy levels (Q-32): «Экономно» and «Максимально».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Economy,
    Maximum,
}

impl Level {
    pub fn kbps(self) -> u32 {
        match self {
            Self::Economy => 128,
            Self::Maximum => 64,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Economy => "economy",
            Self::Maximum => "max",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "economy" => Some(Self::Economy),
            "max" => Some(Self::Maximum),
            _ => None,
        }
    }
}

/// Tags carried into the copy, so it stands on its own on another device: names, order, year,
/// genre and the front cover (Vorbis comments, `METADATA_BLOCK_PICTURE`).
#[derive(Debug, Clone, Default)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    /// JPEG or PNG bytes of the front cover.
    pub cover: Option<Vec<u8>>,
}

/// FLAC picture block (type 3, front cover), base64 — how Ogg carries cover art.
fn picture_block(img: &[u8]) -> String {
    use base64::Engine as _;
    let png = img.starts_with(&[0x89, b'P', b'N', b'G']);
    let mime: &[u8] = if png { b"image/png" } else { b"image/jpeg" };
    let mut b = Vec::with_capacity(img.len() + 64);
    b.extend_from_slice(&3u32.to_be_bytes());
    b.extend_from_slice(&(mime.len() as u32).to_be_bytes());
    b.extend_from_slice(mime);
    b.extend_from_slice(&0u32.to_be_bytes()); // description
    for _ in 0..4 {
        b.extend_from_slice(&0u32.to_be_bytes()); // width, height, depth, colours: unknown
    }
    b.extend_from_slice(&(img.len() as u32).to_be_bytes());
    b.extend_from_slice(img);
    base64::engine::general_purpose::STANDARD.encode(b)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encoded {
    pub duration_ms: u64,
    pub bytes: u64,
}

fn failed(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal).with_details(format!("encode: {e}"))
}

/// The encoder handle, freed on drop.
struct Encoder(*mut op::OpusEncoder);

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: created by `opus_encoder_create`, destroyed once.
        unsafe { op::opus_encoder_destroy(self.0) };
    }
}

impl Encoder {
    fn new(channels: usize, kbps: u32) -> Result<(Self, u32), AppError> {
        let mut err = 0;
        // SAFETY: valid rate/channels/application; `err` receives the status.
        let st = unsafe {
            op::opus_encoder_create(
                RATE as i32,
                channels as i32,
                op::OPUS_APPLICATION_AUDIO,
                &mut err,
            )
        };
        if st.is_null() || err != op::OPUS_OK {
            return Err(failed(format!("opus_encoder_create {err}")));
        }
        let enc = Self(st);
        let mut lookahead: i32 = 0;
        // SAFETY: the documented ctl requests with their argument types.
        unsafe {
            op::opus_encoder_ctl(st, op::OPUS_SET_BITRATE_REQUEST, (kbps * 1000) as i32);
            op::opus_encoder_ctl(st, OPUS_GET_LOOKAHEAD_REQUEST, &mut lookahead as *mut i32);
        }
        Ok((enc, lookahead.max(0) as u32))
    }

    fn encode(&mut self, pcm: &[f32], out: &mut [u8]) -> Result<usize, AppError> {
        let ch = pcm.len() / FRAME;
        debug_assert!(ch == 1 || ch == 2);
        // SAFETY: `pcm` holds exactly FRAME frames, `out` MAX_PACKET bytes.
        let n = unsafe {
            op::opus_encode_float(
                self.0,
                pcm.as_ptr(),
                FRAME as i32,
                out.as_mut_ptr(),
                out.len() as i32,
            )
        };
        if n < 0 {
            return Err(failed(format!("opus_encode_float {n}")));
        }
        Ok(n as usize)
    }
}

fn opus_head(channels: u8, pre_skip: u16, input_rate: u32) -> Vec<u8> {
    let mut h = b"OpusHead".to_vec();
    h.push(1); // version
    h.push(channels);
    h.extend_from_slice(&pre_skip.to_le_bytes());
    h.extend_from_slice(&input_rate.to_le_bytes());
    h.extend_from_slice(&0i16.to_le_bytes()); // output gain
    h.push(0); // mapping family 0: mono/stereo
    h
}

fn opus_tags(tags: &Tags) -> Vec<u8> {
    let vendor = b"SirTone";
    let mut comments: Vec<String> = Vec::new();
    let numbers = [
        ("TRACKNUMBER", tags.track_no.map(|n| n.to_string())),
        ("DISCNUMBER", tags.disc_no.map(|n| n.to_string())),
        ("DATE", tags.year.map(|y| y.to_string())),
    ];
    for (k, v) in [
        ("TITLE", tags.title.clone()),
        ("ARTIST", tags.artist.clone()),
        ("ALBUM", tags.album.clone()),
        ("ALBUMARTIST", tags.album_artist.clone()),
        ("GENRE", tags.genre.clone()),
    ]
    .into_iter()
    .chain(numbers)
    {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            comments.push(format!("{k}={v}"));
        }
    }
    if let Some(img) = tags.cover.as_deref().filter(|i| !i.is_empty()) {
        comments.push(format!("METADATA_BLOCK_PICTURE={}", picture_block(img)));
    }
    let mut t = b"OpusTags".to_vec();
    t.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    t.extend_from_slice(vendor);
    t.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for c in comments {
        t.extend_from_slice(&(c.len() as u32).to_le_bytes());
        t.extend_from_slice(c.as_bytes());
    }
    t
}

/// Mono stays mono; anything wider becomes its front pair (the music the app plays is stereo).
fn to_stereo_or_mono(input: &[f32], ch: usize, out: &mut Vec<f32>) -> usize {
    if ch <= 2 {
        out.extend_from_slice(input);
        return ch.max(1);
    }
    for f in input.chunks_exact(ch) {
        out.push(f[0]);
        out.push(f[1]);
    }
    2
}

/// Encodes `src` into an Ogg Opus copy at `dst` at `level`. `cancel` stops it between packets
/// (the partial file is removed).
pub fn to_opus(
    src: &Path,
    dst: &Path,
    level: Level,
    tags: &Tags,
    cancel: &AtomicBool,
) -> Result<Encoded, AppError> {
    let part = dst.with_extension("opus.part");
    let r = encode_into(src, &part, level, tags, cancel);
    match r {
        Ok(e) => {
            std::fs::rename(&part, dst).map_err(failed)?;
            Ok(e)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

fn encode_into(
    src: &Path,
    part: &Path,
    level: Level,
    tags: &Tags,
    cancel: &AtomicBool,
) -> Result<Encoded, AppError> {
    let mut source = Source::open(src)?;
    let in_rate = source.rate;
    // The first packet tells the real channel count.
    let mut decoded = Vec::new();
    let mut pending: Vec<f32> = Vec::new();
    if !source.next_into(&mut decoded)? {
        return Err(AppError::new(ErrorCode::DecodeFailed).with_details("empty"));
    }
    let ch = to_stereo_or_mono(&decoded, source.channels.max(1), &mut pending);
    // Real input frames (counted as they come from the decoder, never the padding).
    let mut in_frames: u64 = (pending.len() / ch) as u64;

    if let Some(dir) = part.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    let file = std::fs::File::create(part).map_err(failed)?;
    let mut ogg = PacketWriter::new(std::io::BufWriter::new(file));
    let serial = 0x5172_7430;
    let (mut enc, pre_skip) = Encoder::new(ch, level.kbps())?;
    ogg.write_packet(
        opus_head(ch as u8, pre_skip as u16, in_rate),
        serial,
        PacketWriteEndInfo::EndPage,
        0,
    )
    .map_err(failed)?;
    ogg.write_packet(opus_tags(tags), serial, PacketWriteEndInfo::EndPage, 0)
        .map_err(failed)?;

    let mut resampler = if in_rate == RATE {
        None
    } else {
        Some(
            Fft::<f32>::new(in_rate as usize, RATE as usize, CHUNK, ch, FixedSync::Input)
                .map_err(failed)?,
        )
    };
    let mut skip_out = resampler.as_ref().map_or(0, |r| r.output_delay());
    let mut out48: Vec<f32> = Vec::new();
    let mut rbuf = vec![0.0f32; resampler.as_ref().map_or(0, |r| r.output_frames_max()) * ch];
    let mut packet = vec![0u8; MAX_PACKET];
    let mut granule: u64 = u64::from(pre_skip);
    let mut written48: u64 = 0;
    let mut ended = false;
    // The packet before the newest (the last must carry the end flag).
    let mut held: Option<Vec<u8>> = None;

    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::new(ErrorCode::Cancelled));
        }
        if !ended {
            decoded.clear();
            if source.next_into(&mut decoded)? {
                let before = pending.len();
                to_stereo_or_mono(&decoded, source.channels.max(1), &mut pending);
                in_frames += ((pending.len() - before) / ch) as u64;
            } else {
                ended = true;
            }
        }
        // The exact length at 48 kHz.
        let total48 = in_frames * u64::from(RATE) / u64::from(in_rate);
        match resampler.as_mut() {
            None => out48.append(&mut pending),
            Some(r) => loop {
                let need = r.input_frames_next();
                if pending.len() < need * ch {
                    if !ended {
                        break;
                    }
                    // After the end, silence pushes the resampler's delayed tail out.
                    if written48 + (out48.len() / ch) as u64 >= total48 {
                        break;
                    }
                    pending.resize(need * ch, 0.0);
                }
                let input =
                    InterleavedSlice::new(&pending[..need * ch], ch, need).map_err(failed)?;
                let cap = rbuf.len() / ch;
                let mut output = InterleavedSlice::new_mut(&mut rbuf, ch, cap).map_err(failed)?;
                let (_, n) = r
                    .process_into_buffer(&input, &mut output, None)
                    .map_err(failed)?;
                pending.drain(..need * ch);
                let from = skip_out.min(n);
                skip_out -= from;
                out48.extend_from_slice(&rbuf[from * ch..n * ch]);
            },
        }
        if ended {
            // Padding beyond the real length is cut.
            let keep = (total48.saturating_sub(written48) as usize) * ch;
            out48.truncate(keep.min(out48.len()));
        }
        // Whole 20 ms frames; at the end the last one is padded.
        while out48.len() >= FRAME * ch || ended && !out48.is_empty() {
            let real = (out48.len() / ch).min(FRAME);
            let mut frame: Vec<f32> = out48.drain(..real * ch).collect();
            frame.resize(FRAME * ch, 0.0);
            let n = enc.encode(&frame, &mut packet)?;
            if let Some(p) = held.take() {
                ogg.write_packet(p, serial, PacketWriteEndInfo::NormalPacket, granule)
                    .map_err(failed)?;
            }
            written48 += real as u64;
            granule = u64::from(pre_skip) + written48;
            held = Some(packet[..n].to_vec());
        }
        if ended {
            break;
        }
    }
    let last =
        held.ok_or_else(|| AppError::new(ErrorCode::DecodeFailed).with_details("no audio"))?;
    ogg.write_packet(last, serial, PacketWriteEndInfo::EndStream, granule)
        .map_err(failed)?;
    let mut inner = ogg.into_inner();
    inner.flush().map_err(failed)?;
    drop(inner);
    let bytes = std::fs::metadata(part).map_err(failed)?.len();
    Ok(Encoded {
        duration_ms: written48 * 1000 / u64::from(RATE),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16-bit PCM WAV: `secs` of a 440 Hz tone at `rate`, `ch` channels.
    fn wav(path: &Path, rate: u32, ch: u16, secs: u32) {
        let frames = rate * secs;
        let data_len = frames * u32::from(ch) * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&ch.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * u32::from(ch) * 2).to_le_bytes());
        b.extend_from_slice(&(ch * 2).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let v =
                ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12_000.0) as i16;
            for _ in 0..ch {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        std::fs::write(path, b).unwrap();
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("sirtone-encode-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 44.1 kHz stereo → Opus 128: plays back at the same length, far smaller, no `.part` left.
    #[test]
    fn a_copy_plays_back_at_the_same_length() {
        let d = dir("len");
        let (src, dst) = (d.join("t.wav"), d.join("t.opus"));
        wav(&src, 44_100, 2, 3);
        let tags = Tags {
            title: Some("Ёлка".into()),
            artist: Some("Кино".into()),
            track_no: Some(3),
            ..Tags::default()
        };
        let e = to_opus(&src, &dst, Level::Economy, &tags, &AtomicBool::new(false)).unwrap();
        assert!((2_980..=3_020).contains(&e.duration_ms), "{e:?}");
        let original = std::fs::metadata(&src).unwrap().len();
        assert!(e.bytes * 8 < original, "{} vs {original}", e.bytes);
        assert!(!dst.with_extension("opus.part").exists());
        let mut back = Source::open(&dst).unwrap();
        assert_eq!(back.codec, "opus");
        let mut buf = Vec::new();
        let mut frames = 0u64;
        while back.next_into(&mut buf).unwrap() {
            frames += (buf.len() / back.channels.max(1)) as u64;
            buf.clear();
        }
        let ms = frames * 1000 / u64::from(back.rate);
        assert!((2_950..=3_050).contains(&ms), "decoded {ms} ms");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// «Максимально» is smaller than «Экономно»; a cancelled copy leaves nothing behind.
    #[test]
    fn maximum_is_smaller_and_cancel_cleans_up() {
        let d = dir("lvl");
        let src = d.join("t.wav");
        wav(&src, 48_000, 1, 4);
        let none = AtomicBool::new(false);
        let eco = to_opus(
            &src,
            &d.join("e.opus"),
            Level::Economy,
            &Tags::default(),
            &none,
        )
        .unwrap();
        let max = to_opus(
            &src,
            &d.join("m.opus"),
            Level::Maximum,
            &Tags::default(),
            &none,
        )
        .unwrap();
        assert!(max.bytes < eco.bytes, "{max:?} {eco:?}");
        let stop = AtomicBool::new(true);
        assert!(
            to_opus(
                &src,
                &d.join("c.opus"),
                Level::Economy,
                &Tags::default(),
                &stop
            )
            .is_err()
        );
        assert!(!d.join("c.opus").exists() && !d.join("c.opus.part").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
