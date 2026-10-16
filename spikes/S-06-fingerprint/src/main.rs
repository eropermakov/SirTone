//! S-06: `rusty-chromaprint` vs the official Chromaprint (ffmpeg `-f chromaprint`, algorithm 1 =
//! TEST2) and duplicate thresholds (docs/05 §8, D-16).
//!
//! Usage: `cargo run --release -- <corpus dir>` (corpus: `python make-corpus.py <dir>`).

use std::fs::File;
use std::path::{Path, PathBuf};

use rusty_chromaprint::{Configuration, Fingerprinter};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Decodes the file and runs the fingerprinter on interleaved i16, like `fpcalc` (first 120 s).
fn fingerprint(path: &Path) -> Vec<u32> {
    let file = File::open(path).expect("open");
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(e) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(e);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .expect("probe");
    let track = format.default_track(TrackType::Audio).expect("track");
    let id = track.id;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).expect("audio").clone();
    let mut dec = symphonia::default::get_codecs().make_audio_decoder(&params, &AudioDecoderOptions::default()).expect("codec");
    let mut fp = Fingerprinter::new(&Configuration::preset_test2());
    let mut started = false;
    let mut buf: Vec<i16> = Vec::new();
    while let Ok(Some(p)) = format.next_packet() {
        if p.track_id != id {
            continue;
        }
        let Ok(a) = dec.decode(&p) else { continue };
        if !started {
            fp.start(a.spec().rate(), a.spec().channels().count() as u32).expect("start");
            started = true;
        }
        buf.resize(a.samples_interleaved(), 0);
        a.copy_to_slice_interleaved(&mut buf);
        fp.consume(&buf);
    }
    fp.finish();
    fp.fingerprint().to_vec()
}

fn read_raw(path: &Path) -> Vec<u32> {
    std::fs::read(path).expect("fp").chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}

/// Similarity = 1 − bit error rate at the best alignment within ±`max_shift` items
/// (AcoustID-style comparison; one item ≈ 0.124 s).
fn similarity(a: &[u32], b: &[u32], max_shift: i32) -> (f64, i32) {
    let mut best = (0.0, 0);
    for s in -max_shift..=max_shift {
        let (mut bits, mut n) = (0u32, 0u32);
        for (i, &x) in a.iter().enumerate() {
            let j = i as i32 + s;
            if j < 0 || j as usize >= b.len() {
                continue;
            }
            bits += (x ^ b[j as usize]).count_ones();
            n += 32;
        }
        if n >= 32 * 64 {
            let sim = 1.0 - bits as f64 / n as f64;
            if sim > best.0 {
                best = (sim, s);
            }
        }
    }
    best
}

fn pct(mut v: Vec<f64>, p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn main() {
    let dir = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "../../target/spikes/s06-corpus".into()));
    let songs = 50;
    let variants = ["orig.wav", "mp3.mp3", "aac.m4a", "quiet.wav", "trim.wav"];
    let t = std::time::Instant::now();
    let fps: Vec<Vec<Vec<u32>>> = (0..songs)
        .map(|i| variants.iter().map(|v| fingerprint(&dir.join(format!("song{i:02}_{v}")))).collect())
        .collect();
    let secs = t.elapsed().as_secs_f64();
    println!("fingerprinted {} files ({:.0} s of audio each) in {secs:.1} s → {:.0} ms per file", songs * variants.len(), 30.0, secs * 1000.0 / (songs * variants.len()) as f64);

    // 1. Port vs official library on identical input.
    let mut exact = Vec::new();
    let mut lens = 0;
    for i in 0..songs {
        for (k, v) in [(0usize, "orig"), (1, "mp3")] {
            let official = read_raw(&dir.join(format!("song{i:02}_{v}.fp")));
            let ours = &fps[i][k];
            if official.len() != ours.len() {
                lens += 1;
            }
            exact.push(similarity(ours, &official, 0).0);
        }
    }
    println!(
        "\nrusty-chromaprint vs Chromaprint (ffmpeg), same file, no shift: min {:.4}, median {:.4}; length mismatches {lens}/{}",
        pct(exact.clone(), 0.0),
        pct(exact.clone(), 0.5),
        exact.len()
    );

    // 2. Same song (variants vs original) vs different songs (originals vs every other variant).
    println!("\n| pair | min | p5 | median | max |\n|---|---|---|---|---|");
    let mut same_all = Vec::new();
    for (k, v) in variants.iter().enumerate().skip(1) {
        let s: Vec<f64> = (0..songs).map(|i| similarity(&fps[i][0], &fps[i][k], 16).0).collect();
        println!("| same song: orig ↔ {v} | {:.3} | {:.3} | {:.3} | {:.3} |", pct(s.clone(), 0.0), pct(s.clone(), 0.05), pct(s.clone(), 0.5), pct(s.clone(), 1.0));
        same_all.extend(s);
    }
    let mut diff = Vec::new();
    for i in 0..songs {
        for j in 0..songs {
            if i != j {
                for k in 0..variants.len() {
                    diff.push(similarity(&fps[i][0], &fps[j][k], 16).0);
                }
            }
        }
    }
    println!("| different songs ({} pairs) | {:.3} | {:.3} | {:.3} | {:.3} |", diff.len(), pct(diff.clone(), 0.0), pct(diff.clone(), 0.05), pct(diff.clone(), 0.5), pct(diff.clone(), 1.0));

    // 3. Threshold sweep: false duplicates (different songs above T) vs missed (same song below T).
    println!("\n| threshold | missed duplicates | false duplicates |\n|---|---|---|");
    for t in [0.70, 0.75, 0.80, 0.85, 0.90] {
        let missed = same_all.iter().filter(|&&s| s < t).count();
        let false_dup = diff.iter().filter(|&&s| s >= t).count();
        println!("| {t:.2} | {missed}/{} ({:.1} %) | {false_dup}/{} ({:.3} %) |", same_all.len(), 100.0 * missed as f64 / same_all.len() as f64, diff.len(), 100.0 * false_dup as f64 / diff.len() as f64);
    }
}
