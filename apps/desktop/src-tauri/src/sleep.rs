//! Sleep timer (T4.21–T4.23, FR-SLEEP-001…007, `14` §14, `07` §8.1, D-48): the pure
//! `sirtone_playback::sleep::SleepTimer` driven at 4 Hz. Each tick sends the engine the next
//! 250 ms piece of the dB line (the render thread glides linearly in dB), glides «тёплый звук»,
//! mutes UI sounds in the final, and watches for the person waking up: any input in the system
//! (`GetLastInputInfo`, polled only while fading) or in the app (`sleep_wake`). At the end:
//! pause (never stop — the position stays), the volume and the filter back for the next Play,
//! then the after-timer action chosen in the settings (by default nothing).
//!
//! Not persisted: a restart forgets the timer (the spec keeps the position only while the app
//! is open).

use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sirtone_platform::sleep_power::{self, Action, After};
use sirtone_playback::sleep::{Phase, SleepCfg, SleepEvent, SleepKind, SleepTimer};
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::playback::{Playback, PlaybackState, SLEEP_SOFT_MIX};
use crate::settings_store::SettingsStore;

const TICK: Duration = Duration::from_millis(250);
/// The wake glide back to full volume (AT-17: ≤ 1 s).
const WAKE_RAMP_MS: u32 = 1000;
/// «Через N треков»: an upcoming track's length when it is not known yet.
const GUESS_TRACK_MS: u64 = 4 * 60_000;

/// What the UI shows (FR-SLEEP-001/006).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct SleepStatus {
    /// `off` · `running` · `fading` · `final` · `woken` · `ended` (the timer paused playback;
    /// the night vibe fades to black, the screen may sleep)
    pub phase: String,
    /// `minutes` · `track` · `tracks`
    pub kind: Option<String>,
    pub remaining_ms: u32,
    /// Tracks left for «через N треков».
    pub tracks_left: Option<u32>,
}

impl SleepStatus {
    fn off() -> Self {
        Self {
            phase: "off".into(),
            kind: None,
            remaining_ms: 0,
            tracks_left: None,
        }
    }
}

enum Cmd {
    Start(SleepKind),
    Cancel,
    Extend,
    Wake,
}

pub struct Sleep {
    tx: Mutex<Sender<Cmd>>,
    status: Arc<Mutex<SleepStatus>>,
    /// For the sleep wave: t/T of the running timer (`None` — off) and E₀ (T4.22).
    wave: Arc<Mutex<(Option<f32>, Option<f32>)>>,
}

impl Sleep {
    pub fn start(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(SleepStatus::off()));
        let wave = Arc::new(Mutex::new((None, None)));
        let (st, wv) = (status.clone(), wave.clone());
        let _ = std::thread::Builder::new()
            .name("sleep-timer".into())
            .spawn(move || run(&app, &rx, &st, &wv));
        Self {
            tx: Mutex::new(tx),
            status,
            wave,
        }
    }

    fn send(&self, c: Cmd) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(c);
        }
    }

    pub fn status(&self) -> SleepStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| SleepStatus::off())
    }

    /// t/T of the running timer, for the sleep wave's energy curve.
    pub fn fraction(&self) -> Option<f32> {
        self.wave.lock().ok().and_then(|w| w.0)
    }

    pub fn e0(&self) -> Option<f32> {
        self.wave.lock().ok().and_then(|w| w.1)
    }

    pub fn set_e0(&self, e0: f32) {
        if let Ok(mut w) = self.wave.lock() {
            w.1 = Some(e0);
        }
    }

    /// The night vibe lets the screen sleep from the fade on (FR-VIZ-009, FR-SLEEP-007).
    pub fn releases_display(&self) -> bool {
        matches!(self.status().phase.as_str(), "fading" | "final" | "ended")
    }

    /// For the tray: start «Сон 30 мин», or «+15 мин» while woken.
    pub fn tray_action(&self) {
        match self.status().phase.as_str() {
            "off" | "ended" => self.send(Cmd::Start(SleepKind::Minutes(30))),
            _ => self.send(Cmd::Extend),
        }
    }
}

struct Run {
    timer: SleepTimer,
    /// Queue position when started (track kinds count from it).
    from_pos: usize,
    /// `GetLastInputInfo` idle at the last tick (input newer than the fade start wakes).
    idle_prev: u64,
    sfx_muted: bool,
    warm: bool,
    /// The last phase the UI was told.
    told: Option<(Phase, u32)>,
}

fn now_ms(t0: Instant) -> u64 {
    t0.elapsed().as_millis() as u64
}

fn cfg(app: &AppHandle) -> (SleepCfg, bool, After) {
    let doc = app
        .try_state::<Arc<SettingsStore>>()
        .map(|s| s.get())
        .unwrap_or_default();
    let s = &doc["sleep"];
    let fade = s["fade_min"].as_u64().unwrap_or(10) as u32;
    let fin = s["final_s"].as_u64().unwrap_or(30) as u32;
    let after = match s["after"].as_str() {
        Some("screen_off") => After::ScreenOff,
        Some("sleep") => After::Sleep,
        Some("hibernate") => After::Hibernate,
        _ => After::Nothing,
    };
    (
        SleepCfg::new(fade, fin),
        s["warm"].as_bool().unwrap_or(false),
        after,
    )
}

/// When the target track ends, from the snapshot and the queue position.
fn track_end(pb: &Playback, kind: SleepKind, from_pos: usize, now: u64) -> (u64, Option<u32>) {
    let snap = pb.snapshot();
    let left_in_track = u64::from(
        snap.duration_ms
            .unwrap_or(0)
            .saturating_sub(snap.position_ms),
    );
    let n = match kind {
        SleepKind::Tracks(n) => n.max(1),
        _ => 1,
    };
    let done = pb.queue_position().saturating_sub(from_pos) as u32;
    let left = n.saturating_sub(done).max(1);
    let more = u64::from(left - 1) * GUESS_TRACK_MS;
    (
        now + left_in_track + more,
        matches!(kind, SleepKind::Tracks(_)).then_some(left),
    )
}

fn phase_name(p: Phase) -> &'static str {
    match p {
        Phase::Running => "running",
        Phase::Fading => "fading",
        Phase::Final => "final",
        Phase::Woken => "woken",
        Phase::Done => "off",
    }
}

fn kind_name(k: SleepKind) -> &'static str {
    match k {
        SleepKind::Minutes(_) => "minutes",
        SleepKind::EndOfTrack => "track",
        SleepKind::Tracks(_) => "tracks",
    }
}

fn publish(app: &AppHandle, status: &Mutex<SleepStatus>, s: SleepStatus) {
    if let Ok(mut cur) = status.lock() {
        *cur = s.clone();
    }
    let _ = s.emit(app);
    crate::tray::refresh_modes(app);
}

/// Back to normal: full volume at once (we are paused or cancelled), no filter, UI sounds.
fn restore(pb: &Playback, run: &Run) {
    pb.sleep_gain(0.0, 0);
    if run.warm {
        pb.set_warm(None);
    }
    if run.sfx_muted {
        pb.mute_sfx(false);
    }
    SLEEP_SOFT_MIX.store(false, Ordering::Relaxed);
    pb.replan();
}

type WaveShare = Mutex<(Option<f32>, Option<f32>)>;

fn run(app: &AppHandle, rx: &Receiver<Cmd>, status: &Mutex<SleepStatus>, wave: &WaveShare) {
    let t0 = Instant::now();
    let mut cur: Option<Run> = None;
    loop {
        let ended = status.lock().is_ok_and(|s| s.phase == "ended");
        let cmd = match rx.recv_timeout(if cur.is_some() {
            TICK
        } else if ended {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(3600)
        }) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let Some(pb) = app.try_state::<Playback>() else {
            continue;
        };
        let now = now_ms(t0);
        match cmd {
            Some(Cmd::Start(kind)) => {
                if let Some(r) = cur.take() {
                    restore(&pb, &r);
                }
                let (c, warm, _) = cfg(app);
                let from_pos = pb.queue_position();
                let end = match kind {
                    SleepKind::Minutes(_) => None,
                    _ => Some(track_end(&pb, kind, from_pos, now).0),
                };
                SLEEP_SOFT_MIX.store(true, Ordering::Relaxed);
                pb.replan();
                cur = Some(Run {
                    timer: SleepTimer::start(kind, c, now, end),
                    from_pos,
                    idle_prev: sirtone_platform::idle::idle_ms(),
                    sfx_muted: false,
                    warm,
                    told: None,
                });
            }
            Some(Cmd::Cancel) => {
                if let Some(r) = cur.take() {
                    restore(&pb, &r);
                }
                publish(app, status, SleepStatus::off());
                if let Ok(mut w) = wave.lock() {
                    *w = (None, None);
                }
                continue;
            }
            Some(Cmd::Extend) => {
                if let Some(r) = cur.as_mut() {
                    r.timer.extend(now);
                    pb.sleep_gain(0.0, WAKE_RAMP_MS);
                    r.told = None;
                }
            }
            Some(Cmd::Wake) => {
                if let Some(r) = cur.as_mut()
                    && r.timer.input(now)
                {
                    pb.sleep_gain(0.0, WAKE_RAMP_MS);
                }
            }
            None => {}
        }
        let Some(r) = cur.as_mut() else {
            if let Ok(mut w) = wave.lock() {
                *w = (None, None);
            }
            // Playing again after the end: the person is awake, «ended» is over.
            if ended && pb.snapshot().state == PlaybackState::Playing {
                publish(app, status, SleepStatus::off());
            }
            continue;
        };
        if let Ok(mut w) = wave.lock() {
            w.0 = Some(r.timer.fraction(now));
        }
        // The target track's end moves with seeks and skips.
        let (end, tracks_left) = if matches!(r.timer.kind, SleepKind::Minutes(_)) {
            (r.timer.end_ms(), None)
        } else {
            track_end(&pb, r.timer.kind, r.from_pos, now)
        };
        r.timer.set_end(end);
        let phase = r.timer.phase(now);
        // Woken by input anywhere in the system (only polled while it matters).
        if matches!(phase, Phase::Fading | Phase::Final | Phase::Woken) {
            let idle = sirtone_platform::idle::idle_ms();
            if idle < r.idle_prev && r.timer.input(now) {
                pb.sleep_gain(0.0, WAKE_RAMP_MS);
            } else if idle < r.idle_prev {
                r.timer.input(now);
            }
            r.idle_prev = idle;
        } else {
            r.idle_prev = sirtone_platform::idle::idle_ms();
        }
        // A manual pause during the fade: the person is awake and stopped it — the timer ends
        // without the after-timer action (D-48).
        if matches!(phase, Phase::Fading | Phase::Final | Phase::Woken)
            && pb.snapshot().state == PlaybackState::Paused
        {
            if let Some(r) = cur.take() {
                restore(&pb, &r);
            }
            publish(app, status, SleepStatus::off());
            if let Ok(mut w) = wave.lock() {
                *w = (None, None);
            }
            continue;
        }
        match r.timer.tick(now) {
            Some(SleepEvent::Ended) => {
                pb.pause();
                // The pause ramp (≈ 50 ms) finishes under −60 dB, then everything comes back.
                std::thread::sleep(Duration::from_millis(200));
                let after = cfg(app).2;
                if let Some(r) = cur.take() {
                    restore(&pb, &r);
                }
                publish(
                    app,
                    status,
                    SleepStatus {
                        phase: "ended".into(),
                        ..SleepStatus::off()
                    },
                );
                if let Ok(mut w) = wave.lock() {
                    *w = (None, None);
                }
                after_timer(app, after);
                continue;
            }
            Some(SleepEvent::Resumed) | None => {}
        }
        let phase = r.timer.phase(now);
        if phase != Phase::Woken {
            pb.sleep_gain(
                r.timer.gain_db(now + TICK.as_millis() as u64),
                TICK.as_millis() as u32,
            );
        }
        // «Тёплый звук»: 20 kHz → 6 kHz on a log scale through the fade.
        if r.warm {
            let p = r.timer.progress(now);
            pb.set_warm(
                (p > 0.0).then(|| (20_000f32.ln() + (6000f32.ln() - 20_000f32.ln()) * p).exp()),
            );
        }
        let mute = phase == Phase::Final;
        if mute != r.sfx_muted {
            pb.mute_sfx(mute);
            r.sfx_muted = mute;
        }
        // Tell the UI on a phase change and once a second (the remaining time).
        let secs = (r.timer.remaining_ms(now) / 1000) as u32;
        if r.told != Some((phase, secs)) {
            r.told = Some((phase, secs));
            publish(
                app,
                status,
                SleepStatus {
                    phase: phase_name(phase).into(),
                    kind: Some(kind_name(r.timer.kind).into()),
                    remaining_ms: r.timer.remaining_ms(now) as u32,
                    tracks_left,
                },
            );
        }
    }
}

/// The after-timer action (T4.23): only by an explicit setting; the path is chosen from the
/// PC's capabilities. `SIRTONE_SMOKE_DRYPOWER` (tests, only with `SIRTONE_DATA_DIR`) records the
/// decision instead of acting — no test may turn the owner's screen off or put the PC to sleep.
fn after_timer(app: &AppHandle, after: After) {
    let action = sleep_power::plan(after, sleep_power::caps());
    if action == Action::None {
        return;
    }
    if std::env::var_os("SIRTONE_DATA_DIR").is_some()
        && std::env::var_os("SIRTONE_SMOKE_DRYPOWER").is_some()
    {
        if let Ok(dir) = app.path().app_local_data_dir() {
            let _ = std::fs::write(dir.join("sleep-after.txt"), format!("{action:?}"));
        }
        if let Some(d) = std::env::var_os("SIRTONE_DATA_DIR") {
            let _ = std::fs::write(
                std::path::Path::new(&d).join("sleep-after.txt"),
                format!("{action:?}"),
            );
        }
        return;
    }
    match action {
        Action::None => {}
        Action::ScreenOff => {
            if let Some(w) = app.get_webview_window("main")
                && let Ok(h) = w.hwnd()
            {
                sleep_power::screen_off(h.0 as isize);
            }
        }
        Action::Suspend => {
            if !sleep_power::suspend(false) {
                tracing::warn!("sleep timer: suspend refused");
            }
        }
        Action::Hibernate => {
            if !sleep_power::suspend(true) {
                tracing::warn!("sleep timer: hibernate refused");
            }
        }
    }
}

/// Starts the timer: `kind` = `minutes` (with `value` minutes), `track`, `tracks` (`value`).
#[tauri::command]
#[specta::specta]
pub fn sleep_start(sleep: State<'_, Sleep>, kind: String, value: u32) {
    let k = match kind.as_str() {
        "track" => SleepKind::EndOfTrack,
        "tracks" => SleepKind::Tracks(value.clamp(1, 50)),
        _ => SleepKind::Minutes(value.clamp(1, 600)),
    };
    sleep.send(Cmd::Start(k));
}

#[tauri::command]
#[specta::specta]
pub fn sleep_cancel(sleep: State<'_, Sleep>) {
    sleep.send(Cmd::Cancel);
}

/// «+15 мин».
#[tauri::command]
#[specta::specta]
pub fn sleep_extend(sleep: State<'_, Sleep>) {
    sleep.send(Cmd::Extend);
}

/// Any action in the window while the timer fades (the page sends it on pointer and key).
#[tauri::command]
#[specta::specta]
pub fn sleep_wake(sleep: State<'_, Sleep>) {
    sleep.send(Cmd::Wake);
}

#[tauri::command]
#[specta::specta]
pub fn sleep_status(sleep: State<'_, Sleep>) -> SleepStatus {
    sleep.status()
}

/// Which after-timer choices this PC offers (hibernate only if available, Q-17).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SleepCaps {
    pub hibernate: bool,
    /// Modern Standby: «сон» means the screen off and the system left to sleep.
    pub modern_standby: bool,
}

#[tauri::command]
#[specta::specta]
pub fn sleep_caps() -> SleepCaps {
    let c = sleep_power::caps();
    SleepCaps {
        hibernate: c.hibernate,
        modern_standby: c.modern_standby && !c.s3,
    }
}
