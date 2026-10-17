//! «Golden» offline renders (T1.7, NFR-AUD-001): the playback chain rendered to WAV must match the
//! recorded output byte for byte. A change here is a change in what the listener hears — review
//! it, then re-record with `UPDATE_GOLDEN=1 cargo test -p sirtone-audio --test golden`.
//! Fixtures come from `tools/fixtures/make-fixtures.ps1` (ffmpeg, `-bitexact`); without them the
//! test is skipped. WMA is left out: Media Foundation belongs to the OS and changes with updates.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use sirtone_audio::output::DeviceFormat;
use sirtone_audio::render::{render, wav_bytes};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/generated")
        .join(rel)
}

/// FNV-1a 64 — stable across platforms and versions, no dependency.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

struct Case {
    name: &'static str,
    files: &'static [&'static str],
    rate: u32,
    volume: f32,
    /// Expected length in frames (exact: gapless and resampler bookkeeping).
    frames: usize,
}

const CASES: &[Case] = &[
    Case {
        name: "flac-native-44k",
        files: &["formats/flac-16-44.flac"],
        rate: 44_100,
        volume: 1.0,
        frames: 441_000,
    },
    Case {
        name: "wav-resampled-48k",
        files: &["formats/wav-16-44.wav"],
        rate: 48_000,
        volume: 1.0,
        frames: 480_000,
    },
    Case {
        name: "hires-flac-96k-to-48k",
        files: &["formats/flac-24-96.flac"],
        rate: 48_000,
        volume: 1.0,
        frames: 480_000,
    },
    Case {
        name: "aac-trimmed-44k",
        files: &["formats/aac-256.m4a"],
        rate: 44_100,
        volume: 1.0,
        frames: 441_000,
    },
    Case {
        name: "opus-48k",
        files: &["formats/opus-160.opus"],
        rate: 48_000,
        volume: 1.0,
        frames: 480_000,
    },
    Case {
        name: "gapless-wav-chain",
        files: &["signals/gapless-a.wav", "signals/gapless-b.wav"],
        rate: 44_100,
        volume: 1.0,
        frames: 352_800,
    },
    Case {
        name: "gapless-mixed-rate-48k",
        files: &["signals/gapless-a.wav", "signals/gapless-b48.wav"],
        rate: 48_000,
        volume: 1.0,
        frames: 384_000,
    },
    Case {
        name: "volume-half",
        files: &["formats/wav-16-44.wav"],
        rate: 44_100,
        volume: 0.5,
        frames: 441_000,
    },
];

const GOLDEN: &str = include_str!("golden.txt");

#[test]
fn renders_match_the_golden_outputs() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut lines = Vec::new();
    let mut failures = Vec::new();
    for c in CASES {
        let files: Vec<PathBuf> = c.files.iter().map(|f| fixture(f)).collect();
        if files.iter().any(|f| !f.exists()) {
            eprintln!("skip {}: fixtures not generated", c.name);
            return;
        }
        let fmt = DeviceFormat {
            rate: c.rate,
            channels: 2,
        };
        let samples = render(&files, fmt, c.volume).unwrap();
        let frames = samples.len() / 2;
        let hash = format!("{:016x}", fnv(&wav_bytes(&samples, fmt)));
        lines.push(format!("{} {hash} {frames}", c.name));
        if frames != c.frames {
            failures.push(format!("{}: {frames} frames, want {}", c.name, c.frames));
        }
        let want = GOLDEN
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{} ", c.name)))
            .map(|rest| rest.split(' ').next().unwrap_or_default().to_owned());
        if !update && want.as_deref() != Some(hash.as_str()) {
            failures.push(format!("{}: {hash}, golden {want:?}", c.name));
        }
        assert!(samples.iter().all(|s| s.is_finite()), "{}: NaN/inf", c.name);
    }
    if update {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden.txt");
        std::fs::write(path, lines.join("\n") + "\n").unwrap();
        return;
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A gapless pair renders exactly like the one file it was cut from (after the start ramp).
#[test]
fn gapless_chain_equals_the_uncut_file() {
    let (a, b, full) = (
        fixture("signals/gapless-a.wav"),
        fixture("signals/gapless-b.wav"),
        fixture("signals/gapless-full.wav"),
    );
    if !(a.exists() && b.exists() && full.exists()) {
        return;
    }
    let fmt = DeviceFormat {
        rate: 44_100,
        channels: 2,
    };
    let joined = render(&[a, b], fmt, 1.0).unwrap();
    let whole = render(&[full], fmt, 1.0).unwrap();
    assert_eq!(joined.len(), whole.len());
    assert_eq!(joined, whole, "the joint is not sample-exact");
}
