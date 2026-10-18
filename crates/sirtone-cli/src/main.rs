//! sirtone-cli — developer harness (docs/03 §3, docs/04 §1.1): the engine without UI.
//! Console program, so stdout is its interface.
//!
//! ```text
//! sirtone-cli probe <file>...            format, rate, channels, duration
//! sirtone-cli play <file> [--volume V]   play to the default device until the end (Ctrl+C stops)
//! sirtone-cli render <file>... --out <wav>  offline render (T1.7)
//! sirtone-cli soak --minutes N <file>...  soak on the real device (P-13)
//! sirtone-cli import <library_root> [sources...]  import (recover + resume first), P-10 numbers
//! sirtone-cli verify <library_root>      database and disk agree
//! ```
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod library;
mod soak;
mod wave;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use sirtone_audio::decode::Source;
use sirtone_audio::{EngineCmd, EngineEvent, PlayState};

fn usage() -> ExitCode {
    eprintln!(
        "sirtone-cli {}\nusage: sirtone-cli probe <file>... | play <file> [--volume 0..1] | render <file>... --out <wav> [--rate N] [--volume V] | soak --minutes N [--volume V] <file>... | latency [--runs N] <file> | drift --minutes N <file> | import <root> [src]... | verify <root> | wave calendar <year> | smtc [toggle <title>]",
        env!("CARGO_PKG_VERSION")
    );
    ExitCode::from(2)
}

fn probe(files: &[String]) -> ExitCode {
    let mut failed = false;
    for f in files {
        match Source::open(&PathBuf::from(f)) {
            Ok(s) => println!(
                "{f}\t{} Hz\t{} ch\t{}",
                s.rate,
                s.channels,
                s.duration_ms
                    .map_or("? s".into(), |d| format!("{:.3} s", d as f64 / 1000.0))
            ),
            Err(e) => {
                failed = true;
                println!("{f}\terror {:?}\t{}", e.code, e.details.unwrap_or_default());
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Plays `files` as one gapless sequence (T1.5): each next file is handed to the engine in
/// advance with `SetNext`, the engine switches at the exact frame.
fn play(files: &[String], volume: f32) -> ExitCode {
    let (tx, rx) = mpsc::channel();
    let engine = match sirtone_audio::start(Box::new(move |ev| {
        let _ = tx.send(ev);
    })) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("engine: {:?}", e.code);
            return ExitCode::FAILURE;
        }
    };
    engine.send(EngineCmd::SetVolume(volume));
    engine.send(EngineCmd::Load {
        path: PathBuf::from(&files[0]),
        autoplay: true,
    });
    let mut current = 0usize;
    engine.send(EngineCmd::SetNext(files.get(1).map(PathBuf::from)));
    let mut code = ExitCode::SUCCESS;
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(EngineEvent::Snapshot(s)) => {
                if let Some(next) = files.get(current + 1)
                    && s.path.as_deref() == Some(std::path::Path::new(next))
                {
                    current += 1;
                    println!("\n→ {next}");
                    engine.send(EngineCmd::SetNext(
                        files.get(current + 1).map(PathBuf::from),
                    ));
                }
                print!(
                    "\r{:?} {:>8.1} s  underruns {}   ",
                    s.state,
                    s.position_ms as f64 / 1000.0,
                    s.underruns
                );
                if s.state == PlayState::Ended {
                    println!();
                    break;
                }
            }
            Ok(EngineEvent::Error { error, .. }) => {
                println!(
                    "\nerror {:?} {}",
                    error.code,
                    error.details.unwrap_or_default()
                );
                code = ExitCode::FAILURE;
                break;
            }
            Err(_) => {
                println!("\nno events for 5 s");
                code = ExitCode::FAILURE;
                break;
            }
        }
    }
    engine.send(EngineCmd::Shutdown);
    code
}

/// P-11 (`08` §1, «анализ трека ≤ 2 с»): the background pass on files — B0, the fingerprint,
/// B1 and the tagger (B2/B3) in one decode — with the time each took. The duration that places
/// the tagger windows comes from the library row in the app; here from a first plain pass
/// (not timed).
fn analyze(files: &[String]) -> ExitCode {
    let t = std::time::Instant::now();
    let (tagger, rules) = match (
        sirtone_analysis::b2::Tagger::builtin(),
        sirtone_analysis::b3::Rules::builtin(),
    ) {
        (Ok(t), Ok(r)) => (t, r),
        (t, r) => {
            println!("tagger: {:?} {:?}", t.err(), r.err().map(|e| e.to_string()));
            return ExitCode::FAILURE;
        }
    };
    println!("tagger loaded in {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let dec = sirtone_audio::pcm::PcmFiles;
    let mut worst = 0f64;
    for f in files {
        let path = std::path::Path::new(f);
        let duration_ms =
            sirtone_analysis::b0::analyze(&dec, path, None).map_or(0, |b| b.duration_ms);
        let deep = sirtone_analysis::b0::Deep {
            tagger: &tagger,
            rules: &rules,
            duration_ms,
        };
        let t = std::time::Instant::now();
        match sirtone_analysis::b0::analyze(&dec, path, Some(deep)) {
            Ok(b) => {
                let ms = t.elapsed().as_secs_f64() * 1e3;
                worst = worst.max(ms);
                println!(
                    "{f}: {ms:.0} ms — {:.1} s, LUFS {:?}, BPM {:?}, key {:?}, fingerprint {} B",
                    b.duration_ms as f64 / 1000.0,
                    b.lufs_i,
                    b.bpm,
                    b.key_camelot,
                    b.fingerprint.as_ref().map_or(0, Vec::len)
                );
                if let Some(v) = b.vibe {
                    println!(
                        "    energy {:.2} valence {:.2} arousal {:.2} dance {:.2} acoustic {:.2} vocal {:.2} aggr {:.2} bright {:.2}
    moods {:.2?}
    genres {:.2?}",
                        v.energy, v.valence, v.arousal, v.danceability, v.acousticness, v.vocal,
                        v.aggression, v.brightness, v.moods, v.genres
                    );
                }
            }
            Err(e) => println!("{f}: {:?}", e.code),
        }
    }
    println!("worst {worst:.0} ms (P-11: target 2000, limit 4000)");
    if worst <= 4000.0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("probe") if args.len() > 1 => probe(&args[1..]),
        Some("play") if args.len() > 1 => {
            let files: Vec<String> = args[1..]
                .iter()
                .take_while(|a| *a != "--volume")
                .cloned()
                .collect();
            let volume = args
                .iter()
                .position(|a| a == "--volume")
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.5);
            play(&files, volume)
        }
        Some("import") if args.len() > 1 => library::import(&args[1], &args[2..]),
        Some("verify") if args.len() > 1 => library::verify(&args[1]),
        Some("wave") if args.get(1).map(String::as_str) == Some("calendar") && args.len() > 2 => {
            wave::calendar(&args[2])
        }
        Some("hw") => {
            println!("{:?}", sirtone_platform::hardware::hardware());
            ExitCode::SUCCESS
        }
        Some("smtc") if args.get(1).map(String::as_str) == Some("toggle") && args.len() > 2 => {
            match sirtone_platform::smtc::toggle_session(&args[2]) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => {
                    eprintln!("smtc: no such session");
                    ExitCode::FAILURE
                }
                Err(e) => {
                    eprintln!("smtc: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("smtc") => match sirtone_platform::smtc::sessions() {
            // What the Windows 11 media flyout and widgets see (T1.23 check).
            Ok(list) => {
                for s in list {
                    println!("{}	{}	{}	{}", s.app, s.status, s.title, s.artist);
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("smtc: {e}");
                ExitCode::FAILURE
            }
        },
        Some("analyze") if args.len() > 1 => analyze(&args[1..]),
        Some("render") if args.len() > 1 => soak::render(&args[1..]),
        Some("soak") if args.len() > 1 => soak::soak(&args[1..]),
        Some("latency") if args.len() > 1 => soak::latency(&args[1..]),
        Some("drift") if args.len() > 1 => soak::drift(&args[1..]),
        _ => usage(),
    }
}
