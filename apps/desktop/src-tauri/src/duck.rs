//! Ducking (T4.14, FR-PLAY-011, `04` §7): a background thread looks at the other applications'
//! audio sessions every `poll_ms` (`sirtone_platform::sessions`), decides with
//! `sirtone_playback::duck::Ducker` and tells the engine the gain; the render thread glides to
//! it (attack 150 ms, release 1.2 s). Sirtone's own processes and the system sounds never count;
//! the user's exceptions (`playback.duck.ignore`, executable names) neither.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sirtone_playback::duck::{DuckParams, Ducker};
use tauri::{AppHandle, Manager};

use crate::settings_store::SettingsStore;

fn params() -> Option<DuckParams> {
    DuckParams::from_json(include_str!("../../../../assets/duck.json")).ok()
}

pub fn start(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("sirtone-duck".into())
        .spawn(move || {
            let Some(p) = params() else { return };
            sirtone_platform::power::background_thread();
            sirtone_platform::sessions::com_init();
            let t0 = Instant::now();
            let mut d = Ducker::default();
            let mut own = sirtone_platform::sessions::own_tree();
            let mut own_at = Instant::now();
            let mut sent: Option<f32> = None;
            // Foreign pids already checked against our process tree.
            let mut seen = std::collections::HashSet::new();
            loop {
                std::thread::sleep(Duration::from_millis(p.poll_ms));
                let Some(store) = app.try_state::<Arc<SettingsStore>>() else {
                    continue;
                };
                let doc = store.get();
                let s = &doc["playback"]["duck"];
                let enabled = s["enabled"].as_bool().unwrap_or(true);
                let amount = s["amount_db"].as_f64().unwrap_or(-12.0) as f32;
                let sens = p
                    .sensitivity
                    .get(s["sensitivity"].as_str().unwrap_or("medium"))
                    .or_else(|| p.sensitivity.get("medium"))
                    .copied();
                let mut ignore: Vec<String> = s["ignore"]
                    .as_array()
                    .map(|v| {
                        v.iter()
                            .filter_map(|x| x.as_str().map(str::to_lowercase))
                            .collect()
                    })
                    .unwrap_or_default();
                ignore.extend(p.builtin_ignore.iter().cloned());
                // New WebView2 helpers come and go: refresh our process tree now and then.
                if own_at.elapsed() > Duration::from_secs(5) {
                    own = sirtone_platform::sessions::own_tree();
                    own_at = Instant::now();
                }
                let (ducked, who) = match (enabled, sens) {
                    (true, Some(sens)) => {
                        let mut foreign = sirtone_platform::sessions::foreign(&own);
                        // A session of a process not seen before may be ours (WebView2 starts
                        // its audio process on the first sound): look at our tree again.
                        if foreign.iter().any(|f| !seen.contains(&f.pid)) {
                            own = sirtone_platform::sessions::own_tree();
                            own_at = Instant::now();
                            foreign.retain(|f| !own.contains(&f.pid));
                            seen.extend(foreign.iter().map(|f| f.pid));
                        }
                        foreign.retain(|f| !ignore.contains(&f.exe));
                        let loudest = foreign.iter().max_by(|a, b| a.peak.total_cmp(&b.peak));
                        let db = loudest.map_or(-120.0, |f| 20.0 * f.peak.max(1e-6).log10());
                        let now = t0.elapsed().as_millis() as u64;
                        (
                            d.step(now, db, sens, p.hold_ms),
                            loudest.map(|f| f.exe.clone()).unwrap_or_default(),
                        )
                    }
                    _ => {
                        d = Ducker::default();
                        (false, String::new())
                    }
                };
                let want = if ducked { amount.min(0.0) } else { 0.0 };
                if sent != Some(want) {
                    tracing::info!(target: "sirtone::duck", ducked, db = want, by = %who, "duck");
                    if let Some(pb) = app.try_state::<crate::playback::Playback>() {
                        pb.set_duck(want);
                    }
                    sent = Some(want);
                }
            }
        });
}
