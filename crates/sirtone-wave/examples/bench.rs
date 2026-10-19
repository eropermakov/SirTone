//! NFR-PERF-004 gate (T3.24, docs/14 §20): a full rebuild — Thompson sample, stage 1 over the
//! whole catalog, 5 picks — on 50 000 synthetic tracks must stay under 15 ms p95.
//!     cargo run --release -p sirtone-wave --example bench [-- N]
// A command-line gate: its output is the report.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::process::ExitCode;
use std::time::Instant;

use sirtone_core::wave::{ContextSnapshot, Daypart};
use sirtone_wave::engine::{Config, Engine};
use sirtone_wave::synth;

const BUDGET_MS: f64 = 15.0;

fn main() -> ExitCode {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(50_000);
    let now = 1_790_000_000_000i64;
    let tracks = synth::library(n, now, 0.5, 42);
    let Ok(cfg) = Config::builtin() else {
        eprintln!("wave assets do not parse");
        return ExitCode::FAILURE;
    };
    let t0 = Instant::now();
    let history = synth::history(&tracks, now);
    let mut e = Engine::new(cfg, &tracks, &history, now, 1);
    let build_ms = t0.elapsed().as_secs_f64() * 1e3;
    let ctx = ContextSnapshot {
        now_utc_ms: now,
        local_minute: 20 * 60,
        weekday: 2,
        daypart: Daypart::Evening,
        season_w: [0.0, 0.0, 0.0, 1.0],
        weather: None,
    };
    // A refill reloads the catalog; season words are cached by then (T3.30).
    let t1 = Instant::now();
    e.reload(&tracks, &history, now);
    let reload_ms = t1.elapsed().as_secs_f64() * 1e3;
    // «Настроить волну» with four filters is evaluated on every reload (T3.18).
    use sirtone_wave::query::{Activity, Character, Freq, Lang, WaveQuery};
    e.set_query(
        WaveQuery {
            character: vec![Character::Energetic],
            language: vec![Lang::Foreign],
            frequency: vec![Freq::Forgotten],
            activity: vec![Activity::Workout],
            ..WaveQuery::default()
        },
        now,
    );
    let t2 = Instant::now();
    e.reload(&tracks, &history, now);
    let reload_q_ms = t2.elapsed().as_secs_f64() * 1e3;
    e.set_query(WaveQuery::default(), now);
    let mut ms: Vec<f64> = (0..320)
        .map(|i| {
            let t = Instant::now();
            let picked = e.next(&ctx, &[], 0.5, 5, now + i * 1000);
            std::hint::black_box(picked);
            t.elapsed().as_secs_f64() * 1e3
        })
        .skip(20)
        .collect();
    ms.sort_by(f64::total_cmp);
    let p = |q: f64| ms[((ms.len() as f64 * q) as usize).min(ms.len() - 1)];
    println!(
        "wave rebuild, {n} tracks: build {build_ms:.0} ms, reload {reload_ms:.0} ms ({reload_q_ms:.0} ms with a 4-filter query); p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms (budget {BUDGET_MS} ms)",
        p(0.5),
        p(0.95),
        p(1.0)
    );
    if p(0.95) > BUDGET_MS {
        eprintln!("NFR-PERF-004 failed");
        return ExitCode::FAILURE;
    }
    if for_all(n, now, &tracks, &history, &ctx) == ExitCode::FAILURE {
        return ExitCode::FAILURE;
    }
    continuation(100_000)
}

/// «Для всех» (T3.32): the same rebuild with the universality score and its exclusions.
fn for_all(
    n: usize,
    now: i64,
    tracks: &[sirtone_core::wave::WaveTrack],
    history: &sirtone_wave::learn::History,
    ctx: &ContextSnapshot,
) -> ExitCode {
    let Ok(cfg) = Config::builtin_profile("for_all") else {
        eprintln!("for_all profile does not parse");
        return ExitCode::FAILURE;
    };
    let mut e = Engine::new(cfg, tracks, history, now, 1);
    let mut ms: Vec<f64> = (0..220)
        .map(|i| {
            let t = Instant::now();
            std::hint::black_box(e.next(ctx, &[], 0.5, 5, now + i * 1000));
            t.elapsed().as_secs_f64() * 1e3
        })
        .skip(20)
        .collect();
    ms.sort_by(f64::total_cmp);
    let p95 = ms[(ms.len() as f64 * 0.95) as usize];
    println!("«Для всех», {n} tracks: p95 {p95:.2} ms (budget {BUDGET_MS} ms)");
    if p95 > BUDGET_MS {
        eprintln!("NFR-PERF-004 failed for «Для всех»");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// T3.14: the playlist continuation on 100 000 tracks — preparing the library (once per library
/// change) and the 20 picks of the playlist page, which opens within the UX budget (400 ms).
fn continuation(n: usize) -> ExitCode {
    use sirtone_core::wave::SimTrack;
    use sirtone_wave::similar::{SimConfig, Similar};
    const PREVIEW_BUDGET_MS: f64 = 150.0;
    let mut s = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let tracks: Vec<SimTrack> = (0..n)
        .map(|i| {
            let mut blob = (0.02f32).to_le_bytes().to_vec();
            blob.extend((0..64).map(|_| (next() % 200) as u8));
            SimTrack {
                id: i as i64 + 1,
                artist_id: (next() % 5_000) as i64,
                play_count: (next() % 20) as u32,
                bpm: Some(70.0 + (next() % 110) as f32),
                key_camelot: Some(format!(
                    "{}{}",
                    1 + next() % 12,
                    if next() % 2 == 0 { "A" } else { "B" }
                )),
                vae: Some([0.5, 0.4, 0.6]),
                embedding: (i % 4 != 0).then_some(blob),
            }
        })
        .collect();
    let Ok(cfg) = SimConfig::builtin() else {
        eprintln!("continuation config does not parse");
        return ExitCode::FAILURE;
    };
    let t = Instant::now();
    let sim = Similar::build(&tracks, cfg);
    let build_ms = t.elapsed().as_secs_f64() * 1e3;
    let seeds: Vec<i64> = (1..=25).collect();
    let t = Instant::now();
    let picks = sim.continuation(&seeds, &Default::default(), &[], 20);
    let preview_ms = t.elapsed().as_secs_f64() * 1e3;
    println!(
        "continuation, {n} tracks: prepare {build_ms:.0} ms, 20 picks {preview_ms:.1} ms ({} found; budget {PREVIEW_BUDGET_MS} ms)",
        picks.len()
    );
    if preview_ms > PREVIEW_BUDGET_MS {
        eprintln!("continuation preview over budget");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
