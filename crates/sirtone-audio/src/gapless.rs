//! Encoder delay/padding that symphonia does not apply (T1.5, docs/04 §3, S-02):
//! MP4/M4A edit list (`moov/trak/edts/elst`, written by ffmpeg and most tools) with
//! `iTunSMPB` (iTunes) as the fallback; Opus pre-skip from the `OpusHead` (applied after seeks).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Frames to drop at the start and the number of real audio frames (at the file sample rate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trim {
    pub start: u64,
    pub valid: Option<u64>,
}

struct Box {
    kind: [u8; 4],
    body_start: u64,
    end: u64,
}

fn read_box(f: &mut File, at: u64, limit: u64) -> Option<Box> {
    if at + 8 > limit {
        return None;
    }
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut h = [0u8; 8];
    f.read_exact(&mut h).ok()?;
    let mut size = u64::from(u32::from_be_bytes([h[0], h[1], h[2], h[3]]));
    let kind = [h[4], h[5], h[6], h[7]];
    let mut body_start = at + 8;
    if size == 1 {
        let mut ext = [0u8; 8];
        f.read_exact(&mut ext).ok()?;
        size = u64::from_be_bytes(ext);
        body_start += 8;
    } else if size == 0 {
        size = limit - at;
    }
    let end = at.checked_add(size)?;
    (size >= 8 && end <= limit).then_some(Box {
        kind,
        body_start,
        end,
    })
}

fn children(f: &mut File, start: u64, end: u64) -> Vec<Box> {
    let mut out = Vec::new();
    let mut at = start;
    while let Some(b) = read_box(f, at, end) {
        at = b.end;
        out.push(b);
        if out.len() > 4096 {
            break;
        }
    }
    out
}

fn find<'a>(boxes: &'a [Box], kind: &[u8; 4]) -> Option<&'a Box> {
    boxes.iter().find(|b| &b.kind == kind)
}

fn read_at(f: &mut File, at: u64, n: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut v = vec![0u8; n];
    f.read_exact(&mut v).ok()?;
    Some(v)
}

fn be32(b: &[u8]) -> u64 {
    u64::from(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// (version, timescale) from `mvhd`/`mdhd` (same layout up to the timescale).
fn timescale(f: &mut File, b: &Box) -> Option<u64> {
    let head = read_at(f, b.body_start, 32)?;
    let ts = if head[0] == 1 {
        be32(&head[20..24])
    } else {
        be32(&head[12..16])
    };
    (ts > 0).then_some(ts)
}

/// Edit list of the first audio track → trim at `rate`.
fn elst_trim(f: &mut File, len: u64, rate: u32) -> Option<Trim> {
    let top = children(f, 0, len);
    let moov = find(&top, b"moov")?;
    let moov_kids = children(f, moov.body_start, moov.end);
    let movie_ts = timescale(f, find(&moov_kids, b"mvhd")?)?;
    for trak in moov_kids.iter().filter(|b| &b.kind == b"trak") {
        let tk = children(f, trak.body_start, trak.end);
        let Some(mdia) = find(&tk, b"mdia") else {
            continue;
        };
        let md = children(f, mdia.body_start, mdia.end);
        let Some(hdlr) = find(&md, b"hdlr") else {
            continue;
        };
        if read_at(f, hdlr.body_start + 8, 4)?.as_slice() != b"soun" {
            continue;
        }
        let media_ts = timescale(f, find(&md, b"mdhd")?)?;
        let edts = find(&tk, b"edts")?;
        let ed = children(f, edts.body_start, edts.end);
        let elst = find(&ed, b"elst")?;
        let head = read_at(f, elst.body_start, 8)?;
        let (version, count) = (head[0], be32(&head[4..8]));
        let entry_len = if version == 1 { 20 } else { 12 };
        let entries = read_at(f, elst.body_start + 8, (count.min(8) as usize) * entry_len)?;
        // The first non-empty edit (media_time != -1) carries the priming offset.
        for e in entries.chunks_exact(entry_len) {
            let (duration, media_time) = if version == 1 {
                (be64(&e[0..8]), be64(&e[8..16]) as i64)
            } else {
                (
                    be32(&e[0..4]),
                    i64::from(u32::from_be_bytes([e[4], e[5], e[6], e[7]]) as i32),
                )
            };
            if media_time < 0 {
                continue;
            }
            let start = media_time as u64 * u64::from(rate) / media_ts;
            let valid = (duration > 0).then(|| duration * u64::from(rate) / movie_ts);
            return Some(Trim { start, valid });
        }
    }
    None
}

/// `iTunSMPB` (« 00000000 00000840 000001CA 0000000000046E00 …»): delay, padding, sample count.
fn itunsmpb_trim(f: &mut File, len: u64) -> Option<Trim> {
    // The tag is small and lives in `moov/udta/meta/ilst`; a bounded scan of `moov` finds it.
    let top = children(f, 0, len);
    let moov = find(&top, b"moov")?;
    let size = (moov.end - moov.body_start).min(8 << 20) as usize;
    let data = read_at(f, moov.body_start, size)?;
    let at = data.windows(8).position(|w| w == b"iTunSMPB")?;
    let tail = &data[at..(at + 200).min(data.len())];
    let text: String = tail
        .iter()
        .map(|&b| {
            if b.is_ascii_hexdigit() || b == b' ' {
                b as char
            } else {
                '|'
            }
        })
        .collect();
    let fields: Vec<&str> = text.split(['|', ' ']).filter(|s| s.len() >= 8).collect();
    if fields.len() < 3 {
        return None;
    }
    let delay = u64::from_str_radix(fields[1], 16).ok()?;
    let count = u64::from_str_radix(fields[3.min(fields.len() - 1)], 16).ok()?;
    Some(Trim {
        start: delay,
        valid: (count > 0).then_some(count),
    })
}

/// Trim for MP4-family files; `None` for everything else or when the file says nothing.
pub fn mp4_trim(path: &Path, rate: u32) -> Option<Trim> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    if !matches!(ext.as_str(), "m4a" | "mp4" | "m4b" | "aac" | "m4p") {
        return None;
    }
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    elst_trim(&mut f, len, rate)
        .or_else(|| itunsmpb_trim(&mut f, len))
        .filter(|t| t.start > 0 || t.valid.is_some())
}

/// Opus pre-skip (48 kHz frames) from the `OpusHead` in codec extra data.
pub fn opus_pre_skip(extra: Option<&[u8]>) -> u64 {
    match extra {
        Some(h) if h.len() >= 12 && &h[0..8] == b"OpusHead" => {
            u64::from(u16::from_le_bytes([h[10], h[11]]))
        }
        _ => 0,
    }
}
