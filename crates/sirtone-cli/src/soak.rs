//! `render` and `soak` (T1.7, docs/08 §2, P-13).
//!
//! `render <file>... --out <wav> [--rate 48000] [--channels 2] [--volume 1]` — the playback chain
//! offline into a float WAV (gapless across files).
//!
//! `soak --minutes N [--volume V] [--seed S] <file>...` — the real engine on the real output
//! device for N minutes with a random user every few seconds (seek, next, pause/resume, volume,
//! sometimes a burst of seeks). P-13: 0 underruns (counted by the render thread whenever playback
//! should sound but the ring is empty).

use sirtone_audio::engine::NextTrack as EngineNext;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use sirtone_audio::output::DeviceFormat;
use sirtone_audio::{EngineCmd, EngineEvent, PlayState};

fn flag<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

/// Plain file arguments (everything that is neither a flag nor a flag's value).
fn files(args: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a.starts_with("--") {
            skip = true;
        } else {
            out.push(PathBuf::from(a));
        }
    }
    out
}

pub fn render(args: &[String]) -> ExitCode {
    let files = files(args);
    let Some(out) = flag::<String>(args, "--out") else {
        eprintln!("render: --out <wav> is required");
        return ExitCode::from(2);
    };
    let fmt = DeviceFormat {
        rate: flag(args, "--rate").unwrap_or(48_000),
        channels: flag(args, "--channels").unwrap_or(2),
    };
    let volume = flag(args, "--volume").unwrap_or(1.0);
    let started = Instant::now();
    match sirtone_audio::render::render_to_wav(&files, fmt, volume, std::path::Path::new(&out)) {
        Ok(frames) => {
            println!(
                "render: {frames} frames ({:.1} s) at {} Hz in {:.2} s",
                frames as f64 / f64::from(fmt.rate),
                fmt.rate,
                started.elapsed().as_secs_f64()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("render: {:?} {}", e.code, e.details.unwrap_or_default());
            ExitCode::FAILURE
        }
    }
}

/// xorshift64*: a reproducible random user (the seed is printed in the report).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

pub fn soak(args: &[String]) -> ExitCode {
    let files = files(args);
    if files.is_empty() {
        eprintln!("soak: give audio files");
        return ExitCode::from(2);
    }
    let minutes: f64 = flag(args, "--minutes").unwrap_or(10.0);
    let volume: f32 = flag(args, "--volume").unwrap_or(0.0);
    let seed: u64 = flag(args, "--seed").unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64 | 1)
    });
    let mut rng = Rng(seed);
    let (tx, rx) = mpsc::channel();
    let engine = match sirtone_audio::start(Box::new(move |ev| {
        let _ = tx.send(ev);
    })) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("soak: engine {:?}", e.code);
            return ExitCode::FAILURE;
        }
    };
    let path = |i: usize| files[i % files.len()].clone();
    let load = |i: usize| {
        engine.send(EngineCmd::Load {
            path: path(i),
            autoplay: true,
            gain_db: 0.0,
            fade_ms: None,
        });
        engine.send(EngineCmd::SetNext(Some(EngineNext::gapless(
            path(i + 1),
            0.0,
        ))));
    };
    engine.send(EngineCmd::SetVolume(volume));
    let mut idx = 0usize;
    load(idx);
    let end = Instant::now() + Duration::from_secs_f64(minutes * 60.0);
    let mut next_action = Instant::now() + Duration::from_secs(5);
    let (mut actions, mut errors, mut underruns, mut tracks) = (0u64, 0u64, 0u64, 1u64);
    let mut last_report = Instant::now();
    let mut duration_ms = 0u64;
    let mut paused = false;
    let mut last_path: Option<PathBuf> = None;
    println!(
        "soak: {minutes} min, {} files, volume {volume}, seed {seed}",
        files.len()
    );
    while Instant::now() < end {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(EngineEvent::Snapshot(s)) => {
                underruns = underruns.max(s.underruns);
                duration_ms = s.duration_ms.unwrap_or(duration_ms);
                if s.path != last_path {
                    // Gapless switch to the handed-over next track.
                    if last_path.is_some() && s.path.as_ref() == Some(&path(idx + 1)) {
                        idx += 1;
                        tracks += 1;
                        engine.send(EngineCmd::SetNext(Some(EngineNext::gapless(
                            path(idx + 1),
                            0.0,
                        ))));
                    }
                    last_path.clone_from(&s.path);
                }
                if s.state == PlayState::Ended {
                    idx += 1;
                    tracks += 1;
                    load(idx);
                }
            }
            Ok(EngineEvent::Error { error, .. }) => {
                errors += 1;
                eprintln!(
                    "soak: error {:?} {}",
                    error.code,
                    error.details.unwrap_or_default()
                );
            }
            Err(_) => {}
        }
        if Instant::now() >= next_action {
            actions += 1;
            match rng.below(100) {
                0..=39 => engine.send(EngineCmd::Seek(rng.below(duration_ms.max(1000)))),
                40..=59 => {
                    idx += 1;
                    tracks += 1;
                    load(idx);
                }
                60..=74 => {
                    engine.send(if paused {
                        EngineCmd::Play
                    } else {
                        EngineCmd::Pause
                    });
                    paused = !paused;
                }
                75..=89 => {
                    let v = volume * (rng.below(101) as f32 / 100.0);
                    engine.send(EngineCmd::SetVolume(v));
                }
                _ => {
                    // A burst: what an impatient user does with the timeline.
                    for _ in 0..10 {
                        engine.send(EngineCmd::Seek(rng.below(duration_ms.max(1000))));
                    }
                }
            }
            if paused && rng.below(2) == 0 {
                engine.send(EngineCmd::Play);
                paused = false;
            }
            next_action = Instant::now() + Duration::from_millis(2_000 + rng.below(15_000));
        }
        if last_report.elapsed() >= Duration::from_secs(60) {
            last_report = Instant::now();
            println!(
                "soak: {:.0} min left — actions {actions}, tracks {tracks}, underruns {underruns}, errors {errors}",
                end.saturating_duration_since(Instant::now()).as_secs_f64() / 60.0
            );
        }
    }
    engine.send(EngineCmd::Shutdown);
    let ok = underruns == 0 && errors == 0;
    println!(
        "soak done: {minutes} min, actions {actions}, tracks {tracks}, underruns {underruns}, errors {errors} → {}",
        if ok { "OK" } else { "FAIL" }
    );
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn p95(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(((v.len() as f64 * 0.95).ceil() as usize).saturating_sub(1))
        .copied()
        .unwrap_or(f64::NAN)
}

/// `latency [--runs 10] <file>` — P-02: «play» → first audible sample, cold and warm.
pub fn latency(args: &[String]) -> ExitCode {
    let Some(file) = files(args).into_iter().next() else {
        eprintln!("latency: give an audio file (a tone works best)");
        return ExitCode::from(2);
    };
    let runs = flag(args, "--runs").unwrap_or(10);
    match sirtone_audio::latency::measure(&file, runs) {
        Ok(mut l) => {
            let fmt = |v: &[f64]| {
                v.iter()
                    .map(|x| format!("{x:.0}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            println!("cold ms: {}", fmt(&l.cold_ms));
            println!("warm ms: {}", fmt(&l.warm_ms));
            let (c, w) = (p95(&mut l.cold_ms), p95(&mut l.warm_ms));
            let ok = c <= 150.0 && w <= 60.0;
            println!(
                "P-02 p95: cold {c:.0} ms (≤ 150), warm {w:.0} ms (≤ 60), {}/{} runs heard → {}",
                l.cold_ms.len().min(l.warm_ms.len()),
                runs,
                if ok { "OK" } else { "OVER TARGET" }
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("latency: {:?} {}", e.code, e.details.unwrap_or_default());
            ExitCode::FAILURE
        }
    }
}

/// `drift --minutes N <long file>` — T1.4: reported position against the wall clock while
/// playing (target ≤ 20 ms/h). Every snapshot gives (wall time, position); the slope of their
/// difference is the drift, so the 250 ms snapshot cadence does not matter.
pub fn drift(args: &[String]) -> ExitCode {
    let Some(file) = files(args).into_iter().next() else {
        eprintln!("drift: give a long audio file");
        return ExitCode::from(2);
    };
    let minutes: f64 = flag(args, "--minutes").unwrap_or(10.0);
    let (tx, rx) = mpsc::channel();
    let engine = match sirtone_audio::start(Box::new(move |ev| {
        let _ = tx.send((Instant::now(), ev));
    })) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("drift: engine {:?}", e.code);
            return ExitCode::FAILURE;
        }
    };
    engine.send(EngineCmd::SetVolume(0.0));
    engine.send(EngineCmd::Load {
        path: file,
        autoplay: true,
        gain_db: 0.0,
        fade_ms: None,
    });
    let start = Instant::now();
    let end = start + Duration::from_secs_f64(minutes * 60.0);
    // (seconds since start, position − wall in ms), after a 3 s settle.
    let mut points: Vec<(f64, f64)> = Vec::new();
    while Instant::now() < end {
        if let Ok((at, EngineEvent::Snapshot(s))) = rx.recv_timeout(Duration::from_millis(500))
            && s.state == PlayState::Playing
        {
            let t = at.duration_since(start).as_secs_f64();
            if t > 3.0 {
                points.push((t, s.position_ms as f64 - t * 1000.0));
            }
        }
    }
    engine.send(EngineCmd::Shutdown);
    if points.len() < 10 {
        eprintln!("drift: not enough snapshots");
        return ExitCode::FAILURE;
    }
    // Least-squares slope of (position − wall) over time: ms per second → ms per hour.
    let n = points.len() as f64;
    let (sx, sy) = points
        .iter()
        .fold((0.0, 0.0), |(a, b), (x, y)| (a + x, b + y));
    let (mx, my) = (sx / n, sy / n);
    let (num, den) = points.iter().fold((0.0, 0.0), |(a, b), (x, y)| {
        (a + (x - mx) * (y - my), b + (x - mx).powi(2))
    });
    let per_hour = num / den * 3600.0;
    let jitter = points
        .iter()
        .map(|(x, y)| (y - (my + num / den * (x - mx))).abs())
        .fold(0.0, f64::max);
    println!(
        "drift: {per_hour:+.1} ms/h over {minutes} min ({} snapshots, jitter ±{jitter:.0} ms) → {}",
        points.len(),
        if per_hour.abs() <= 20.0 {
            "OK"
        } else {
            "OVER 20 ms/h"
        }
    );
    ExitCode::SUCCESS
}
