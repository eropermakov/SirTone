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
        gain_db: 0.0,
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
        gain_db: 0.0,
    });
    engine.send(EngineCmd::Load {
        path: good,
        autoplay: true,
        gain_db: 0.0,
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

fn start_with(path: PathBuf) -> (sirtone_audio::EngineHandle, Arc<Mutex<Option<Snapshot>>>) {
    let last = Arc::new(Mutex::new(None::<Snapshot>));
    let l = last.clone();
    let engine = sirtone_audio::start(Box::new(move |ev| {
        if let EngineEvent::Snapshot(s) = ev {
            *l.lock().unwrap() = Some(s);
        }
    }))
    .unwrap();
    engine.send(EngineCmd::SetVolume(0.2));
    engine.send(EngineCmd::Load {
        path,
        autoplay: true,
        gain_db: 0.0,
    });
    (engine, last)
}

/// T1.6: the default device changed while ours is still alive → move and keep playing from the
/// same position (FR-PLAY-012: «смена default → продолжение ≤ 1 с»).
#[test]
#[ignore = "needs an audio device"]
fn default_change_keeps_playing_from_the_same_place() {
    let Some(path) = fixture("long/mix-3h.mp3") else {
        return;
    };
    let (engine, last) = start_with(path);
    let before = wait_for(&last, Duration::from_secs(3), |s| {
        s.state == PlayState::Playing && s.position_ms > 1000
    });
    engine.send(EngineCmd::DefaultDeviceChanged);
    std::thread::sleep(Duration::from_millis(700));
    engine.send(EngineCmd::SetVolume(0.2)); // forces a snapshot
    let after = wait_for(&last, Duration::from_secs(1), |_| true);
    assert_eq!(after.state, PlayState::Playing);
    assert!(
        after.position_ms + 300 >= before.position_ms,
        "{} → {}",
        before.position_ms,
        after.position_ms
    );
    assert!(
        after.position_ms < before.position_ms + 1500,
        "no jump: {} → {}",
        before.position_ms,
        after.position_ms
    );
    let t = after.position_ms;
    std::thread::sleep(Duration::from_millis(800));
    engine.send(EngineCmd::SetVolume(0.2));
    let later = wait_for(&last, Duration::from_secs(1), |_| true);
    assert!(
        later.position_ms > t + 400,
        "still advancing: {t} → {}",
        later.position_ms
    );
    engine.send(EngineCmd::Shutdown);
}

/// T1.6: our device disappears (pulled headphones) → pause at the same position; Play resumes there.
#[test]
#[ignore = "needs an audio device"]
fn device_loss_pauses_and_play_resumes_there() {
    let Some(path) = fixture("long/mix-3h.mp3") else {
        return;
    };
    let (engine, last) = start_with(path);
    let s = wait_for(&last, Duration::from_secs(3), |s| {
        s.state == PlayState::Playing && s.position_ms > 1000
    });
    engine.send(EngineCmd::DeviceLost("{not-ours}".into()));
    std::thread::sleep(Duration::from_millis(300));
    engine.send(EngineCmd::SetVolume(0.2));
    assert_eq!(
        wait_for(&last, Duration::from_secs(1), |_| true).state,
        PlayState::Playing,
        "other device ignored"
    );
    engine.send(EngineCmd::DeviceLost(s.device.clone().unwrap()));
    let paused = wait_for(&last, Duration::from_secs(1), |s| {
        s.state == PlayState::Paused
    });
    assert!(paused.position_ms + 300 >= s.position_ms);
    engine.send(EngineCmd::Play);
    let resumed = wait_for(&last, Duration::from_secs(2), |s| {
        s.state == PlayState::Playing
    });
    assert!(
        resumed.position_ms.abs_diff(paused.position_ms) < 600,
        "{} vs {}",
        resumed.position_ms,
        paused.position_ms
    );
    engine.send(EngineCmd::Shutdown);
}

/// With «пауза при потере» off the music follows the new default instead.
#[test]
#[ignore = "needs an audio device"]
fn device_loss_without_pause_keeps_playing() {
    let Some(path) = fixture("long/mix-3h.mp3") else {
        return;
    };
    let (engine, last) = start_with(path);
    engine.send(EngineCmd::SetPauseOnDeviceLoss(false));
    let s = wait_for(&last, Duration::from_secs(3), |s| {
        s.state == PlayState::Playing && s.position_ms > 1000
    });
    engine.send(EngineCmd::DeviceLost(s.device.clone().unwrap()));
    std::thread::sleep(Duration::from_millis(800));
    engine.send(EngineCmd::SetVolume(0.2));
    let after = wait_for(&last, Duration::from_secs(1), |_| true);
    assert_eq!(after.state, PlayState::Playing);
    assert!(after.position_ms >= s.position_ms);
    engine.send(EngineCmd::Shutdown);
}
