//! Background analysis (T3.6 B0 + T3.10 scheduler, FR-PERF-005, docs/04 §10): one track at a
//! time on a below-normal, EcoQoS thread with its own connection; newest tracks first; paused
//! on battery and while a full-screen game runs; on «Очень слабое» only while nobody is at the
//! computer. Files are only read. The wave picks the results up on its next refill.
//! B2/B3 (T3.8/T3.9): the built-in tagger runs in the same pass; it is loaded when there is work
//! and dropped when the queue is empty (≈ 20 MB of weights).

use std::path::PathBuf;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sirtone_core::error::AppError;
use tauri::{AppHandle, Manager, State};

use crate::library::{Library, db_path, medium};
use crate::settings_store::SettingsStore;

const VERSION: i64 = sirtone_analysis::b0::VERSION;
/// Idle time before analysis runs on the weakest preset (the same rule as updates, D-76).
const IDLE_MS: u64 = 2 * 60 * 1000;

fn level(app: &AppHandle) -> u64 {
    app.try_state::<std::sync::Arc<SettingsStore>>()
        .and_then(|s| s.get()["performance"]["level"].as_u64())
        .unwrap_or(3)
}

/// Isolated test runs (`SIRTONE_SMOKE_AC` together with `SIRTONE_DATA_DIR`) analyse on battery
/// too: a laptop unplugged during the smoke must not stop the analysis checks.
pub(crate) fn battery_ignored() -> bool {
    std::env::var_os("SIRTONE_SMOKE_AC").is_some() && std::env::var_os("SIRTONE_DATA_DIR").is_some()
}

/// Why analysis waits now, if it does.
fn blocked(app: &AppHandle) -> bool {
    (sirtone_platform::power::on_battery() && !battery_ignored())
        || sirtone_platform::power::fullscreen_busy()
        || (level(app) == 0 && sirtone_platform::idle::idle_ms() < IDLE_MS)
}

pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("sirtone-analysis".into())
        .spawn(move || {
            sirtone_platform::power::background_thread();
            // Let the start settle (library open, first screen) before reading files.
            std::thread::sleep(Duration::from_secs(20));
            let mut conn: Option<(PathBuf, Connection)> = None;
            let mut deep: Option<Deep> = None;
            loop {
                let pause = step(&app, &mut conn, &mut deep);
                std::thread::sleep(pause);
            }
        })
        .ok();
}

/// The tagger, the B3 rules and the vibe membership functions (mood bits, activities).
struct Deep {
    tagger: sirtone_analysis::b2::Tagger,
    rules: sirtone_analysis::b3::Rules,
    vibe: sirtone_wave::query::Vibe,
}

impl Deep {
    fn load() -> Option<Self> {
        let loaded = (|| {
            Ok::<_, String>(Self {
                tagger: sirtone_analysis::b2::Tagger::builtin()?,
                rules: sirtone_analysis::b3::Rules::builtin().map_err(|e| e.to_string())?,
                vibe: sirtone_wave::query::Vibe::builtin().map_err(|e| e.to_string())?,
            })
        })();
        loaded
            .map_err(|e| tracing::error!(target: "sirtone::analysis", error = %e, "tagger"))
            .ok()
    }

    /// The wave vector and the SQL filter columns of a track with a vibe.
    fn store(
        &self,
        c: &Connection,
        track: i64,
        b: &sirtone_core::analysis::B0,
    ) -> Result<(), sirtone_library::LibraryError> {
        let Some(v) = &b.vibe else {
            return Ok(());
        };
        let x = sirtone_wave::engine::deep_features(v, b.bpm);
        let acts = self.vibe.activities(&x);
        let json = serde_json::to_string(
            &sirtone_library::smart::ACTIVITIES
                .iter()
                .zip(acts)
                .map(|(k, v)| (*k, (v * 100.0).round() / 100.0))
                .collect::<std::collections::BTreeMap<_, _>>(),
        )
        .unwrap_or_default();
        sirtone_library::analysis::store_features(
            c,
            track,
            &x,
            sirtone_core::wave::FEATURES_SCHEMA,
            v.conf,
            self.vibe.mood_bits(&x),
            &json,
        )
    }
}

/// One unit of work; returns how long to wait before the next.
fn step(
    app: &AppHandle,
    slot: &mut Option<(PathBuf, Connection)>,
    deep: &mut Option<Deep>,
) -> Duration {
    if blocked(app) {
        return Duration::from_secs(30);
    }
    let Some(root) = app.try_state::<Library>().and_then(|l| l.root()) else {
        return Duration::from_secs(30);
    };
    if slot.as_ref().is_none_or(|(r, _)| *r != root) {
        match sirtone_library::open_existing(&db_path(&root), medium(&root)) {
            Ok(c) => *slot = Some((root.clone(), c)),
            Err(_) => return Duration::from_secs(60),
        }
    }
    let Some((_, c)) = slot.as_ref() else {
        return Duration::from_secs(60);
    };
    let next = match sirtone_library::analysis::pending(c, VERSION, 1) {
        Ok(v) => v.into_iter().next(),
        Err(_) => {
            *slot = None;
            return Duration::from_secs(60);
        }
    };
    let Some((track, rel, duration_ms)) = next else {
        // Everything analysed: free the tagger, look again in a while (imports add tracks).
        *deep = None;
        return Duration::from_secs(120);
    };
    if deep.is_none() {
        *deep = Deep::load();
        // Without the tagger the tracks would be stamped with this version and never get a
        // vibe: wait and try again instead.
        if deep.is_none() {
            return Duration::from_secs(600);
        }
    }
    let path = rel.split('/').fold(root, |p, part| p.join(part));
    let t0 = std::time::Instant::now();
    let d = deep.as_ref().map(|d| sirtone_analysis::b0::Deep {
        tagger: &d.tagger,
        rules: &d.rules,
        duration_ms,
    });
    let stored = match sirtone_analysis::b0::analyze(&sirtone_audio::pcm::PcmFiles, &path, d) {
        Ok(b) => sirtone_library::analysis::store_b0(c, track, &b, VERSION).and_then(|()| {
            if let Some(d) = deep.as_ref() {
                d.store(c, track, &b)?;
            }
            // T3.5: a fresh fingerprint is compared with the library (duplicates by sound).
            if b.fingerprint.is_some() && sirtone_library::duplicates::detect(c, track)? > 0 {
                crate::duplicates::found(app, sirtone_library::duplicates::pending_count(c)?);
            }
            Ok(())
        }),
        Err(e) => {
            tracing::warn!(target: "sirtone::analysis", track, code = ?e.code, "B0 failed");
            // FR-IMP-008: a library file the decoder no longer reads (imported playable, so it
            // broke since) goes to the quarantine — the user decides; nothing is deleted.
            let broken = matches!(
                e.code,
                sirtone_core::error::ErrorCode::UnsupportedFormat
                    | sirtone_core::error::ErrorCode::DecodeFailed
            );
            sirtone_library::analysis::mark_failed(c, track, VERSION).and_then(|()| {
                if broken
                    && sirtone_library::unknowns::quarantine(c, track)?
                    && let Some(lib) = app.try_state::<Library>()
                {
                    lib.notify_changed(app);
                }
                Ok(())
            })
        }
    };
    if stored.is_err() {
        *slot = None;
    }
    tracing::debug!(target: "sirtone::analysis", track, ms = t0.elapsed().as_millis() as u64, "B0");
    // A breath between tracks keeps the machine responsive.
    Duration::from_millis(300)
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisProgress {
    pub done: u32,
    pub total: u32,
    /// Waiting for power, a game or idle time.
    pub paused: bool,
}

#[tauri::command(async)]
#[specta::specta]
pub fn analysis_progress(
    app: AppHandle,
    lib: State<'_, Library>,
) -> Result<AnalysisProgress, AppError> {
    let (done, total) = lib.with_read(|c| sirtone_library::analysis::progress(c, VERSION))?;
    Ok(AnalysisProgress {
        done,
        total,
        paused: blocked(&app),
    })
}
