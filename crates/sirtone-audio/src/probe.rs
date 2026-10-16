//! `MediaProbe` for the import pipeline (docs/05 §3 «Probed»): opens the file with the same
//! decoder layer that plays it, so «imported» always means «playable».

use std::path::Path;

use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::ports::{MediaInfo, MediaProbe};

use crate::decode::Source;

pub struct SymphoniaProbe;

fn lossless(codec: &str) -> bool {
    codec.starts_with("pcm") || matches!(codec, "flac" | "alac" | "wavpack" | "ape" | "tta")
}

impl MediaProbe for SymphoniaProbe {
    fn probe(&self, path: &Path) -> Result<MediaInfo, AppError> {
        let mut source = Source::open(path)?;
        let duration_ms = match source.duration_ms {
            Some(d) => d,
            None => {
                // No length in the container: decoding one packet proves it plays; length 0 is
                // filled by analysis later.
                let mut probe = Vec::new();
                if !source.next_into(&mut probe)? {
                    return Err(AppError::new(ErrorCode::DecodeFailed).with_details("no audio"));
                }
                0
            }
        };
        let codec = if source.codec.is_empty() {
            "unknown".to_string()
        } else {
            source.codec.clone()
        };
        Ok(MediaInfo {
            container: path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default(),
            lossless: lossless(&codec),
            codec,
            sample_rate: Some(source.rate),
            channels: u32::try_from(source.channels).ok(),
            duration_ms,
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

    #[test]
    fn probes_formats_and_rejects_junk() {
        let cases = [
            ("formats/flac-24-96.flac", "flac", true),
            ("formats/mp3-cbr320.mp3", "mp3", false),
            ("formats/alac-16-44.m4a", "alac", true),
            ("formats/aac-256.m4a", "aac", false),
            ("formats/opus-160.opus", "opus", false),
        ];
        for (rel, codec, lossless) in cases {
            let Some(p) = fixture(rel) else { return };
            let info = SymphoniaProbe.probe(&p).unwrap();
            assert_eq!(info.codec, codec, "{rel}");
            assert_eq!(info.lossless, lossless, "{rel}");
            assert!(info.duration_ms >= 9_900, "{rel}: {}", info.duration_ms);
        }
        if let Some(p) = fixture("broken/text-as.mp3") {
            assert_eq!(
                SymphoniaProbe.probe(&p).err().map(|e| e.code),
                Some(ErrorCode::UnsupportedFormat)
            );
        }
    }
}
