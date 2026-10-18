//! Audio fingerprints (T3.2, FR-REC-001; `05` §4–§6, D-16, spike S-06): Chromaprint via the
//! `rusty-chromaprint` port (equivalent to the official library, ≥ 0.996 in S-06) over the first
//! 120 s, stored in Chromaprint's own compressed format — the one AcoustID takes (base64url) —
//! and compared like AcoustID: 1 − bit error rate at the best alignment (the format and the
//! comparison are in `sirtone_core::fingerprint`).

use rusty_chromaprint::{Configuration, Fingerprinter};
pub use sirtone_core::fingerprint::{base64url, compress, decompress, similarity, sketch};

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

    /// The compressed fingerprint, its sketch and the duration it covers (ms); `None` without
    /// audio.
    pub fn finish(mut self) -> Option<(Vec<u8>, Vec<u8>, u64)> {
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
            sketch(raw),
            self.frames * 1000 / u64::from(self.rate.max(1)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
