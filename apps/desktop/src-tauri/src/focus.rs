//! Focus (T4.24, FR-FOCUS-001…006/008, `14` §15, `01c` W-09): the pure
//! `sirtone_playback::focus::FocusCycle` at 2 Hz. Work plays the `focus` wave, breaks the
//! `focus_break` wave; the wave packs tracks so the last one ends at the boundary (§7.1). At a
//! boundary: a soft UI sound (Windows Clock), the wave switches profile — the current track
//! finishes if it ends within the allowed overrun, otherwise «Смешать сейчас» crossfades into
//! the new music. Normalization is switched on for the session if it was off and given back
//! afterwards (unless changed by hand meanwhile). The day's count is kept in `focus-today.json`
//! (today only, no history).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sirtone_core::wave::Clock;
use sirtone_platform::clock::SystemClock;
use sirtone_playback::focus::{FocusCfg, FocusCycle, FocusEvent, Segment};
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::playback::Playback;
use crate::settings_store::SettingsStore;

const TICK: Duration = Duration::from_millis(500);

/// What the UI and the tray show (FR-FOCUS-002).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FocusStatus {
    /// `off` · `work` · `break` · `long_break`
    pub phase: String,
    pub remaining_ms: u32,
    /// The segment's full length (the ring).
    pub segment_ms: u32,
    pub paused: bool,
    /// Work segments done today.
    pub today: u32,
}

impl FocusStatus {
    fn off(today: u32) -> Self {
        Self {
            phase: "off".into(),
            remaining_ms: 0,
            segment_ms: 0,
            paused: false,
            today,
        }
    }
}

enum Cmd {
    /// Minutes of work and break (`None` — the settings).
    Start(Option<(u32, u32)>),
    Stop,
    Pause,
    Resume,
    Skip,
}

pub struct Focus {
    tx: Mutex<Sender<Cmd>>,
    status: Arc<Mutex<FocusStatus>>,
    /// The current segment's end in `Instant` terms for packing (`None` — off or paused).
    segment: Arc<Mutex<Option<(String, Instant)>>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Today {
    day: i32,
    count: u32,
}

fn today_file(data: &std::path::Path) -> PathBuf {
    data.join("focus-today.json")
}

fn local_day() -> i32 {
    let c = SystemClock;
    sirtone_wave::clock::local_time(&c, c.now_utc_ms()).day
}

fn load_today(data: &std::path::Path) -> u32 {
    std::fs::read(today_file(data))
        .ok()
        .and_then(|b| serde_json::from_slice::<Today>(&b).ok())
        .filter(|t| t.day == local_day())
        .map_or(0, |t| t.count)
}

fn save_today(data: &std::path::Path, count: u32) {
    if let Ok(b) = serde_json::to_vec(&Today {
        day: local_day(),
        count,
    }) {
        let _ = std::fs::write(today_file(data), b);
    }
}

impl Focus {
    pub fn start(app: AppHandle, data: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(FocusStatus::off(load_today(&data))));
        let segment = Arc::new(Mutex::new(None));
        let (st, sg) = (status.clone(), segment.clone());
        let _ = std::thread::Builder::new()
            .name("focus".into())
            .spawn(move || run(&app, &rx, &st, &sg, &data));
        Self {
            tx: Mutex::new(tx),
            status,
            segment,
        }
    }

    fn send(&self, c: Cmd) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(c);
        }
    }

    pub fn status(&self) -> FocusStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| FocusStatus::off(0))
    }

    /// The wave profile of the running segment and when it ends (packing, §7.1).
    pub fn segment(&self) -> Option<(String, Instant)> {
        self.segment.lock().ok().and_then(|s| s.clone())
    }

    /// For the tray: start with the settings, or stop.
    pub fn tray_action(&self) {
        if self.status().phase == "off" {
            // The line reads «Фокус 25/5».
            self.send(Cmd::Start(Some((25, 5))));
        } else {
            self.send(Cmd::Stop);
        }
    }
}

fn settings(app: &AppHandle) -> serde_json::Value {
    app.try_state::<Arc<SettingsStore>>()
        .map(|s| s.get())
        .unwrap_or_default()
}

fn cfg_of(doc: &serde_json::Value, pick: Option<(u32, u32)>) -> (FocusCfg, u64) {
    let f = &doc["focus"];
    let g = |k: &str, d: u64| f[k].as_u64().unwrap_or(d) as u32;
    let (work, brk) = pick.unwrap_or((g("work_min", 25), g("break_min", 5)));
    // A preset's long break: three times the short one (25/5 → 15, as Pomodoro).
    let long = if pick.is_some() {
        brk * 3
    } else {
        g("long_break_min", 15)
    };
    (
        FocusCfg::minutes(work, brk, long, g("cycles_before_long", 4)),
        u64::from(g("overrun_s", 90)) * 1000,
    )
}

fn profile_of(s: Segment) -> &'static str {
    match s {
        Segment::Work => "focus",
        Segment::Break | Segment::LongBreak => "focus_break",
    }
}

fn phase_of(s: Segment) -> &'static str {
    match s {
        Segment::Work => "work",
        Segment::Break => "break",
        Segment::LongBreak => "long_break",
    }
}

struct Run {
    cycle: FocusCycle,
    cfg: FocusCfg,
    seg_ms: u64,
    overrun_ms: u64,
    /// Normalization was switched on for the session (FR-FOCUS-004): give it back after.
    norm_restore: bool,
    told: Option<(Segment, u32, bool)>,
}

fn publish(app: &AppHandle, status: &Mutex<FocusStatus>, s: FocusStatus) {
    if let Ok(mut cur) = status.lock() {
        *cur = s.clone();
    }
    let _ = s.emit(app);
    crate::tray::refresh_modes(app);
}

/// The segment's music: the wave continues with the new profile after the current track; if
/// that track would run past the allowed overrun, the next one is mixed in now (≈ 4 s).
fn switch_music(app: &AppHandle, segment: Segment, overrun_ms: u64) {
    if let Some(w) = app.try_state::<crate::wave::Wave>() {
        w.switch_profile(profile_of(segment));
    }
    if let Some(pb) = app.try_state::<Playback>() {
        let s = pb.snapshot();
        let left = u64::from(s.duration_ms.unwrap_or(0).saturating_sub(s.position_ms));
        if left > overrun_ms {
            // Give the wave a moment to queue the new segment's first track.
            std::thread::sleep(Duration::from_millis(600));
            pb.mix_now();
        }
    }
}

fn end(app: &AppHandle, r: &Run) {
    if r.norm_restore {
        restore_norm(app);
    }
}

/// Normalization back to off (unless switched off by hand meanwhile) and the marker cleared.
fn restore_norm(app: &AppHandle) {
    let Some(s) = app.try_state::<Arc<SettingsStore>>() else {
        return;
    };
    let mut patch = serde_json::json!({ "focus": { "norm_restore": false } });
    if s.get()["playback"]["normalization"]["enabled"].as_bool() == Some(true) {
        patch["playback"] = serde_json::json!({ "normalization": { "enabled": false } });
    }
    let _ = s.patch(&patch);
}

fn run(
    app: &AppHandle,
    rx: &Receiver<Cmd>,
    status: &Mutex<FocusStatus>,
    segment: &Mutex<Option<(String, Instant)>>,
    data: &std::path::Path,
) {
    let t0 = Instant::now();
    let now_ms = || t0.elapsed().as_millis() as u64;
    // The app quit during a focus session that had switched normalization on: give it back.
    if settings(app)["focus"]["norm_restore"].as_bool() == Some(true) {
        restore_norm(app);
    }
    let mut cur: Option<Run> = None;
    loop {
        let cmd = match rx.recv_timeout(if cur.is_some() {
            TICK
        } else {
            Duration::from_secs(3600)
        }) {
            Ok(c) => Some(c),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let now = now_ms();
        let day = local_day();
        match cmd {
            Some(Cmd::Start(pick)) => {
                if let Some(r) = cur.take() {
                    end(app, &r);
                }
                let doc = settings(app);
                let (cfg, overrun_ms) = cfg_of(&doc, pick);
                // FR-FOCUS-004: normalization on for the session.
                let mut norm_restore = false;
                if doc["focus"]["auto_normalize"].as_bool().unwrap_or(true)
                    && doc["playback"]["normalization"]["enabled"].as_bool() == Some(false)
                    && let Some(s) = app.try_state::<Arc<SettingsStore>>()
                {
                    // The marker goes with it: a quit mid-session still gives «off» back.
                    norm_restore = s
                        .patch(&serde_json::json!({
                            "playback": { "normalization": { "enabled": true } },
                            "focus": { "norm_restore": true }
                        }))
                        .is_ok();
                }
                let cycle = FocusCycle::start(cfg, now, day, load_today(data));
                if let Ok(mut s) = segment.lock() {
                    *s = Some((
                        "focus".into(),
                        Instant::now() + Duration::from_millis(cfg.work_ms),
                    ));
                }
                if let Some(w) = app.try_state::<crate::wave::Wave>() {
                    w.start_profile("focus");
                }
                cur = Some(Run {
                    cycle,
                    cfg,
                    seg_ms: cfg.work_ms,
                    overrun_ms,
                    norm_restore,
                    told: None,
                });
            }
            Some(Cmd::Stop) => {
                if let Some(r) = cur.take() {
                    end(app, &r);
                }
                if let Ok(mut s) = segment.lock() {
                    *s = None;
                }
                publish(app, status, FocusStatus::off(load_today(data)));
                continue;
            }
            Some(Cmd::Pause) => {
                if let Some(r) = cur.as_mut() {
                    r.cycle.pause(now);
                }
            }
            Some(Cmd::Resume) => {
                if let Some(r) = cur.as_mut() {
                    r.cycle.resume(now);
                }
            }
            Some(Cmd::Skip) => {
                if let Some(r) = cur.as_mut() {
                    let ev = r.cycle.skip(now, day);
                    boundary(app, r, ev, data);
                }
            }
            None => {}
        }
        let Some(r) = cur.as_mut() else {
            continue;
        };
        if let Some(ev) = r.cycle.tick(now, day) {
            boundary(app, r, ev, data);
        }
        if let Ok(mut s) = segment.lock() {
            *s = r.cycle.end_ms().map(|e| {
                (
                    profile_of(r.cycle.segment).to_owned(),
                    Instant::now() + Duration::from_millis(e.saturating_sub(now)),
                )
            });
        }
        let secs = (r.cycle.remaining_ms(now) / 1000) as u32;
        let key = (r.cycle.segment, secs, r.cycle.paused());
        if r.told != Some(key) {
            r.told = Some(key);
            publish(
                app,
                status,
                FocusStatus {
                    phase: phase_of(r.cycle.segment).into(),
                    remaining_ms: r.cycle.remaining_ms(now) as u32,
                    segment_ms: r.seg_ms as u32,
                    paused: r.cycle.paused(),
                    today: r.cycle.today(day),
                },
            );
        }
    }
}

fn boundary(app: &AppHandle, r: &mut Run, ev: FocusEvent, data: &std::path::Path) {
    let sound = match ev {
        FocusEvent::WorkEnded => {
            save_today(data, r.cycle.today(local_day()));
            "focus_work_end"
        }
        FocusEvent::BreakEnded => "focus_break_end",
    };
    if let Some(pb) = app.try_state::<Playback>() {
        pb.sfx(sound);
    }
    r.seg_ms = match r.cycle.segment {
        Segment::Work => r.cfg.work_ms,
        Segment::Break => r.cfg.break_ms,
        Segment::LongBreak => r.cfg.long_break_ms,
    };
    switch_music(app, r.cycle.segment, r.overrun_ms);
}

/// Starts focus: `work`/`brk` minutes (a preset) or `0` — the settings.
#[tauri::command]
#[specta::specta]
pub fn focus_start(focus: State<'_, Focus>, work: u32, brk: u32) {
    let pick = (work > 0 && brk > 0).then(|| (work.clamp(5, 180), brk.clamp(1, 60)));
    focus.send(Cmd::Start(pick));
}

#[tauri::command]
#[specta::specta]
pub fn focus_stop(focus: State<'_, Focus>) {
    focus.send(Cmd::Stop);
}

#[tauri::command]
#[specta::specta]
pub fn focus_pause(focus: State<'_, Focus>, paused: bool) {
    focus.send(if paused { Cmd::Pause } else { Cmd::Resume });
}

/// «Пропустить сегмент».
#[tauri::command]
#[specta::specta]
pub fn focus_skip(focus: State<'_, Focus>) {
    focus.send(Cmd::Skip);
}

#[tauri::command]
#[specta::specta]
pub fn focus_status(focus: State<'_, Focus>) -> FocusStatus {
    focus.status()
}
