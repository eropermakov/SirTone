//! Engine on the real default device (needs audio hardware → `#[ignore]`; run with
//! `cargo nextest run -p sirtone-audio --run-ignored all`). Plays quietly (volume 0.3 → −31 dB).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test harness

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sirtone_audio::{EngineCmd, EngineEvent, PlayState, Snapshot};

fn fixture(rel: &str) -> Option<PathBuf> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/generated")
        .join(rel);
    p.exists().then_some(p)
}

fn wait_for(
    last: &Arc<Mutex<Option<Snapshot>>>,
    timeout: Duration,
    pred: impl Fn(&Snapshot) -> bool,
) -> Snapshot {
    let t0 = Instant::now();
    loop {
        if let Some(s) = last.lock().unwrap().clone()
            && pred(&s)
        {
            return s;
        }
        assert!(
            t0.elapsed() < timeout,
            "timeout; last = {:?}",
            last.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "needs an audio device"]
fn plays_seeks_pauses_and_ends_on_device() {
    let Some(path) = fixture("formats/flac-16-44.flac") else {
        return;
    };
    let last = Arc::new(Mutex::new(None::<Snapshot>));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let (l, e) = (last.clone(), errors.clone());
    let engine = sirtone_audio::start(Box::new(move |ev| match ev {
        EngineEvent::Snapshot(s) => *l.lock().unwrap() = Some(s),
        EngineEvent::Error { error, .. } => e.lock().unwrap().push(error),
    }))
    .unwrap();
    engine.send(EngineCmd::SetVolume(0.3));
    engine.send(EngineCmd::Load {
        path,
        autoplay: true,
    });

    // Clock: after ~1.5 s of wall time the position is ~1.5 s (latency-compensated).
    wait_for(&last, Duration::from_secs(3), |s| {
        s.state == PlayState::Playing
    });
    let t0 = Instant::now();
    std::thread::sleep(Duration::from_millis(1500));
    let s = wait_for(&last, Duration::from_secs(1), |_| true);
    let wall = t0.elapsed().as_millis() as i64;
    assert_eq!(s.duration_ms, Some(10_000));
    assert!(
        (s.position_ms as i64 - wall).abs() < 400,
        "pos {} vs wall {wall}",
        s.position_ms
    );

    // Seek near the end, then the track must end by itself.
    engine.send(EngineCmd::Seek(8_500));
    let s = wait_for(&last, Duration::from_secs(1), |s| s.position_ms >= 8_500);
    assert!(s.position_ms < 9_200, "seek landed at {}", s.position_ms);

    engine.send(EngineCmd::Pause);
    let paused = wait_for(&last, Duration::from_secs(1), |s| {
        s.state == PlayState::Paused
    });
    std::thread::sleep(Duration::from_millis(500));
    engine.send(EngineCmd::SetVolume(0.3)); // forces a snapshot
    let still = wait_for(&last, Duration::from_secs(1), |_| true);
    assert!(
        still.position_ms.abs_diff(paused.position_ms) < 60,
        "position moved while paused"
    );

    engine.send(EngineCmd::Play);
    let end = wait_for(&last, Duration::from_secs(4), |s| {
        s.state == PlayState::Ended
    });
    assert_eq!(end.position_ms, 10_000);
    assert_eq!(end.underruns, 0);
    assert!(
        errors.lock().unwrap().is_empty(),
        "{:?}",
        errors.lock().unwrap()
    );
    engine.send(EngineCmd::Shutdown);
}

#[test]
#[ignore = "needs an audio device"]
fn broken_file_reports_error_and_engine_survives() {
    let (Some(bad), Some(good)) = (
        fixture("broken/text-as.mp3"),
        fixture("formats/mp3-cbr320.mp3"),
    ) else {
        return;
    };
    let last = Arc::new(Mutex::new(None::<Snapshot>));
    let errors = Arc::new(Mutex::new(Vec::new()));
    let (l, e) = (last.clone(), errors.clone());
    let engine = sirtone_audio::start(Box::new(move |ev| match ev {
        EngineEvent::Snapshot(s) => *l.lock().unwrap() = Some(s),
        EngineEvent::Error { error, .. } => e.lock().unwrap().push(error.code),
    }))
    .unwrap();
    engine.send(EngineCmd::SetVolume(0.2));
    engine.send(EngineCmd::Load {
        path: bad,
        autoplay: true,
    });
    engine.send(EngineCmd::Load {
        path: good,
        autoplay: true,
    });
    let s = wait_for(&last, Duration::from_secs(3), |s| {
        s.state == PlayState::Playing && s.position_ms > 300
    });
    assert!(s.path.is_some());
    assert_eq!(
        errors.lock().unwrap().as_slice(),
        [sirtone_core::error::ErrorCode::UnsupportedFormat]
    );
    engine.send(EngineCmd::Shutdown);
}
