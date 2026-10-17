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
    let mut e = Engine::new(cfg, &tracks, now, 1);
    let build_ms = t0.elapsed().as_secs_f64() * 1e3;
    let ctx = ContextSnapshot {
        now_utc_ms: now,
        local_minute: 20 * 60,
        weekday: 2,
        daypart: Daypart::Evening,
        season_w: [0.0, 0.0, 0.0, 1.0],
        weather: None,
    };
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
        "wave rebuild, {n} tracks: build {build_ms:.0} ms; p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms (budget {BUDGET_MS} ms)",
        p(0.5),
        p(0.95),
        p(1.0)
    );
    if p(0.95) > BUDGET_MS {
        eprintln!("NFR-PERF-004 failed");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
