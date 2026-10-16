//! sirtone-cli — developer harness (docs/03 §3, docs/04 §1.1): the engine without UI.
//! Console program, so stdout is its interface.
//!
//! ```text
//! sirtone-cli probe <file>...            format, rate, channels, duration
//! sirtone-cli play <file> [--volume V]   play to the default device until the end (Ctrl+C stops)
//! sirtone-cli render <file> --out <wav>  offline render (T1.7, not yet)
//! sirtone-cli import <library_root> [sources...]  import (recover + resume first), P-10 numbers
//! sirtone-cli verify <library_root>      database and disk agree
//! ```
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod library;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use sirtone_audio::decode::Source;
use sirtone_audio::{EngineCmd, EngineEvent, PlayState};

fn usage() -> ExitCode {
    eprintln!(
        "sirtone-cli {}\nusage: sirtone-cli probe <file>... | play <file> [--volume 0..1] | render <file> --out <wav>",
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
        Some("render") => {
            eprintln!("render: offline WAV render arrives with T1.7");
            ExitCode::FAILURE
        }
        _ => usage(),
    }
}
