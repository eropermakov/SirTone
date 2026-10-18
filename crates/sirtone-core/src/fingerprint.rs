//! Chromaprint fingerprints as data (T3.2/T3.5; `05` §4–§6, D-16): the compressed format
//! AcoustID takes, base64url, and the comparison used for duplicates. Pure functions — the
//! fingerprinting itself (decoding, `rusty-chromaprint`) lives in `sirtone-analysis`.
//!
//! The compressed format (from Chromaprint's `fingerprint_compressor.cpp`, MIT; the idea, not
//! the code): a 4-byte header (algorithm, 24-bit item count, big-endian), then for each item
//! XOR the previous one, the 1-based distances between its set bits, each item ended by 0;
//! those values as a little-endian stream of 3-bit numbers (7 = «7 or more»), then the excess
//! (value − 7) of those as 5-bit numbers.

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

/// Sketch stride and size: every 8th item, at most 128 (≈ 2 min of a 120 s fingerprint).
pub const SKETCH_STRIDE: usize = 8;
const SKETCH_ITEMS: usize = 128;

/// Every 8th raw item (u32 LE) — stored next to the fingerprint for the coarse check.
pub fn sketch(fp: &[u32]) -> Vec<u8> {
    fp.iter()
        .step_by(SKETCH_STRIDE)
        .take(SKETCH_ITEMS)
        .flat_map(|v| v.to_le_bytes())
        .collect()
}

pub fn sketch_items(blob: &[u8]) -> Vec<u32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect()
}

/// A cheap first look: the best similarity of a full fingerprint against another's sketch
/// (all shifts ±16). Different songs stay well below 0.7 (S-06: ≤ 0.682), so a full comparison
/// is needed only above that.
pub fn similarity_coarse(full: &[u32], sketch: &[u32]) -> f32 {
    const SHIFT: i32 = 16;
    let mut best = 0f32;
    for s in -SHIFT..=SHIFT {
        let (mut bits, mut n) = (0u32, 0u32);
        for (k, &x) in sketch.iter().enumerate() {
            let j = (k * SKETCH_STRIDE) as i32 + s;
            if j < 0 || j as usize >= full.len() {
                continue;
            }
            bits += (x ^ full[j as usize]).count_ones();
            n += 1;
        }
        if n > 0 {
            best = best.max(1.0 - bits as f32 / (n * 32) as f32);
        }
    }
    best
}

/// Whether a fingerprint says enough to compare: silence and drones give a few repeating items
/// that would «match» any other silence (S-06 saw such fixtures). At least `min_distinct`
/// different items and some bit activity between neighbours.
pub fn informative(fp: &[u32], min_distinct: usize) -> bool {
    if fp.len() < 64 {
        return false;
    }
    let mut seen: Vec<u32> = fp.to_vec();
    seen.sort_unstable();
    seen.dedup();
    let changes: u32 = fp.windows(2).map(|w| (w[0] ^ w[1]).count_ones()).sum();
    seen.len() >= min_distinct && changes as usize >= fp.len() * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALGORITHM: u8 = 1;

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

    #[test]
    fn silence_and_drones_are_not_informative() {
        assert!(!informative(&[0u32; 300], 16));
        assert!(!informative(&[0x1234_5678u32; 300], 16));
        let varied: Vec<u32> = (0..300u32).map(|i| i.wrapping_mul(2_654_435_761)).collect();
        assert!(informative(&varied, 16));
    }
}
