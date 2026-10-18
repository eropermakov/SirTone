//! Audio fingerprints (T3.2, FR-REC-001; `05` §4–§6, D-16, spike S-06): Chromaprint via the
//! `rusty-chromaprint` port (equivalent to the official library, ≥ 0.996 in S-06) over the first
//! 120 s, stored in Chromaprint's own compressed format — the one AcoustID takes (base64url) —
//! and compared like AcoustID: 1 − bit error rate at the best alignment.
//!
//! The compressed format (from Chromaprint's `fingerprint_compressor.cpp`, MIT; the idea, not
//! the code): a 4-byte header (algorithm, 24-bit item count, big-endian), then for each item
//! XOR the previous one, the 1-based distances between its set bits, each item ended by 0;
//! those values as a little-endian stream of 3-bit numbers (7 = «7 or more»), then the excess
//! (value − 7) of those as 5-bit numbers.

use rusty_chromaprint::{Configuration, Fingerprinter};

/// Chromaprint algorithm 1 (TEST2), as `fpcalc` and AcoustID by default.
pub const ALGORITHM: u8 = 1;
/// AcoustID uses the first 120 s.
pub const SECONDS: u32 = 120;

/// Collects the first 120 s of decoded audio (interleaved f32) into a fingerprint.
pub struct FpAcc {
    fp: Fingerprinter,
    started: bool,
    left: u64,
    buf: Vec<i16>,
    channels: usize,
    rate: u32,
    frames: u64,
    failed: bool,
}

impl Default for FpAcc {
    fn default() -> Self {
        Self {
            fp: Fingerprinter::new(&Configuration::preset_test2()),
            started: false,
            left: 0,
            buf: Vec::new(),
            channels: 0,
            rate: 0,
            frames: 0,
            failed: false,
        }
    }
}

impl FpAcc {
    pub fn feed(&mut self, frames: &[f32], channels: usize, rate: u32) {
        if self.failed {
            return;
        }
        if !self.started {
            if self.fp.start(rate, channels as u32).is_err() {
                self.failed = true;
                return;
            }
            self.started = true;
            self.channels = channels.max(1);
            self.rate = rate;
            self.left = u64::from(rate) * u64::from(SECONDS);
        }
        if self.left == 0 {
            return;
        }
        let n = ((frames.len() / self.channels) as u64).min(self.left) as usize;
        self.buf.clear();
        self.buf.extend(
            frames[..n * self.channels]
                .iter()
                .map(|v| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16),
        );
        self.fp.consume(&self.buf);
        self.left -= n as u64;
        self.frames += n as u64;
    }

    /// The compressed fingerprint and the duration it covers (ms); `None` without audio.
    pub fn finish(mut self) -> Option<(Vec<u8>, u64)> {
        if !self.started || self.failed {
            return None;
        }
        self.fp.finish();
        let raw = self.fp.fingerprint();
        if raw.is_empty() {
            return None;
        }
        Some((
            compress(raw, ALGORITHM),
            self.frames * 1000 / u64::from(self.rate.max(1)),
        ))
    }
}

/// Little-endian stream of `width`-bit values.
fn pack(values: &[u8], width: u32, out: &mut Vec<u8>) {
    let (mut acc, mut bits) = (0u32, 0u32);
    for &v in values {
        acc |= u32::from(v) << bits;
        bits += width;
        while bits >= 8 {
            out.push(acc as u8);
            acc >>= 8;
            bits -= 8;
        }
    }
    if bits > 0 {
        out.push(acc as u8);
    }
}

fn unpack(data: &[u8], width: u32, count: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(count);
    let (mut acc, mut bits, mut i) = (0u32, 0u32, 0usize);
    let mask = (1u32 << width) - 1;
    while out.len() < count {
        while bits < width {
            acc |= u32::from(*data.get(i)?) << bits;
            i += 1;
            bits += 8;
        }
        out.push((acc & mask) as u8);
        acc >>= width;
        bits -= width;
    }
    Some(out)
}

/// Chromaprint's compressed fingerprint.
pub fn compress(fp: &[u32], algorithm: u8) -> Vec<u8> {
    let mut normal: Vec<u8> = Vec::with_capacity(fp.len() * 8);
    let mut prev = 0u32;
    for &item in fp {
        let mut x = item ^ prev;
        prev = item;
        let (mut bit, mut last) = (1u8, 0u8);
        while x != 0 {
            if x & 1 != 0 {
                normal.push(bit - last);
                last = bit;
            }
            x >>= 1;
            bit += 1;
        }
        normal.push(0);
    }
    let exceptional: Vec<u8> = normal.iter().filter(|&&v| v >= 7).map(|v| v - 7).collect();
    let n = fp.len() as u32;
    let mut out = vec![algorithm, (n >> 16) as u8, (n >> 8) as u8, n as u8];
    let capped: Vec<u8> = normal.iter().map(|&v| v.min(7)).collect();
    pack(&capped, 3, &mut out);
    pack(&exceptional, 5, &mut out);
    out
}

/// Back to the raw items (`None` when the data is broken).
pub fn decompress(data: &[u8]) -> Option<Vec<u32>> {
    let n = (usize::from(*data.get(1)?) << 16)
        | (usize::from(*data.get(2)?) << 8)
        | usize::from(*data.get(3)?);
    let body = data.get(4..)?;
    // Read 3-bit values until `n` items are closed by their zeros.
    let mut normal = Vec::new();
    let (mut acc, mut bits, mut i, mut items) = (0u32, 0u32, 0usize, 0usize);
    while items < n {
        while bits < 3 {
            acc |= u32::from(*body.get(i)?) << bits;
            i += 1;
            bits += 8;
        }
        let v = (acc & 7) as u8;
        acc >>= 3;
        bits -= 3;
        if v == 0 {
            items += 1;
        }
        normal.push(v);
    }
    let used = (normal.len() * 3).div_ceil(8);
    let ex_count = normal.iter().filter(|&&v| v == 7).count();
    let ex = unpack(body.get(used..)?, 5, ex_count)?;
    let mut ex = ex.into_iter();
    let mut out = Vec::with_capacity(n);
    let (mut x, mut last, mut prev) = (0u32, 0u32, 0u32);
    for v in normal {
        if v == 0 {
            prev ^= x;
            out.push(prev);
            x = 0;
            last = 0;
            continue;
        }
        let d = if v == 7 {
            7 + u32::from(ex.next()?)
        } else {
            u32::from(v)
        };
        last += d;
        if last == 0 || last > 32 {
            return None;
        }
        x |= 1 << (last - 1);
    }
    Some(out)
}

/// URL-safe base64 without padding (what AcoustID and `fpcalc` print).
pub fn base64url(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let v = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..=c.len() {
            s.push(char::from(A[((v >> (18 - 6 * k)) & 63) as usize]));
        }
    }
    s
}

/// Similarity of two fingerprints: 1 − bit error rate at the best alignment within ±16 items
/// (≈ ±2 s), over at least 64 overlapping items (S-06), with the overlap share of the shorter.
pub fn similarity(a: &[u32], b: &[u32]) -> Option<(f32, f32)> {
    const SHIFT: i32 = 16;
    let mut best: Option<(f32, f32)> = None;
    for s in -SHIFT..=SHIFT {
        let (mut bits, mut n) = (0u32, 0u32);
        for (i, &x) in a.iter().enumerate() {
            let j = i as i32 + s;
            if j < 0 || j as usize >= b.len() {
                continue;
            }
            bits += (x ^ b[j as usize]).count_ones();
            n += 1;
        }
        if n >= 64 {
            let sim = 1.0 - bits as f32 / (n * 32) as f32;
            let overlap = n as f32 / a.len().min(b.len()).max(1) as f32;
            if best.is_none_or(|(b, _)| sim > b) {
                best = Some((sim, overlap));
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_round_trips() {
        let mut fp = Vec::new();
        let mut x = 0x1234_5678u32;
        for i in 0..900u32 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            // Some items equal to the previous (empty XOR), some with high bits (exceptions).
            fp.push(if i % 17 == 0 {
                *fp.last().unwrap_or(&0)
            } else {
                x
            });
        }
        fp.push(0x8000_0001);
        let c = compress(&fp, ALGORITHM);
        assert_eq!(c[0], ALGORITHM);
        assert_eq!(decompress(&c).unwrap(), fp);
        assert!(decompress(&c[..c.len() / 2]).is_none());
    }

    /// Parity with the official Chromaprint (ffmpeg 9.0.1 `-f chromaprint`): the raw items and
    /// the base64 fingerprint of the same file — our compressor gives the same text.
    #[test]
    fn compressed_form_matches_chromaprint() {
        let raw: Vec<u32> = include_bytes!("../tests/data/fp-music.raw")
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let want = include_str!("../tests/data/fp-music.b64").trim();
        assert_eq!(base64url(&compress(&raw, ALGORITHM)), want);
    }

    #[test]
    fn similarity_of_shifted_and_different() {
        let a: Vec<u32> = (0..400u32).map(|i| i.wrapping_mul(2_654_435_761)).collect();
        let shifted: Vec<u32> = a[5..].to_vec();
        let (s, o) = similarity(&a, &shifted).unwrap();
        assert!(s > 0.999 && o > 0.99, "{s} {o}");
        let b: Vec<u32> = (0..400u32)
            .map(|i| i.wrapping_mul(40_503).rotate_left(7))
            .collect();
        assert!(similarity(&a, &b).unwrap().0 < 0.7);
        assert!(similarity(&a[..10], &b).is_none(), "too short to compare");
    }
}
