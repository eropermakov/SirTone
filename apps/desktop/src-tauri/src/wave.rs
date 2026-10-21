//! «Моя волна» in the app (Stage 3, docs/14 §2.2): the engine lives on its own `sirtone-wave`
//! thread. `wave_start` builds it from the library (statistics, likes, «никогда», «не сейчас»)
//! and plays the first tracks; the playback sink asks for more when fewer than two are ahead,
//! and every refill reloads the catalog — the plays, skips and likes since then count at once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::wave::Clock;
use sirtone_platform::clock::SystemClock;
use sirtone_wave::engine::{Config, Engine};
use sirtone_wave::learn::History;
use sirtone_wave::query::{Activity, Character, Freq, Lang, Mood, SeasonPick, WaveQuery};
use tauri::{AppHandle, Manager, State};

use crate::library::Library;
use crate::playback::{PlayContext, Playback, items_for};
use crate::settings_store::SettingsStore;

/// Tracks queued when the wave starts, and per refill (docs/14 §7: 3–5 ahead).
const FIRST: usize = 5;
const MORE: usize = 3;

enum Cmd {
    /// A new wave with a profile (`my_wave`, `seasonal`) and «Настроить волну».
    Start(String, WaveQuery, Sender<Result<WaveStarted, AppError>>),
    More,
    Why(i64, Sender<Vec<WhyReason>>),
    /// The slider moved: the upcoming wave tracks are picked again.
    Retune,
    /// A wave track was skipped: its vibe repels and the upcoming tracks are picked again
    /// (D-92, AT-21).
    Skipped(i64),
    /// Another profile from the next track on (focus segments).
    Switch(String),
    /// «Подходит N треков» for «Настроить волну».
    Count(WaveQuery, Sender<Option<WaveCount>>),
    /// «Состав волны»: an influence switched on or off for the session.
    Off,
    /// What the library has for an event (direct matches, by tone) — the card's gate.
    EventFit(String, Sender<Option<sirtone_wave::events::Fit>>),
}

/// One reason of «Почему этот трек» (§17); the UI words it as `wave.why.<kind>`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WhyReason {
    /// liked · genre · taste · context · session · new · forgotten · season
    pub kind: String,
    /// Genre group (0…5), daypart code or season (0 winter … 3 autumn).
    pub arg: Option<u8>,
}

/// «Настроить волну» (T3.18, docs/05 §10.1) as the UI sends it: chip keys per block; empty or
/// absent — no filter.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct WaveQueryIpc {
    /// happy · upbeat · calm · sad · positive · negative
    pub mood: Vec<String>,
    /// energetic · aggressive · calm
    pub character: Vec<String>,
    /// ru · foreign · none
    pub language: Vec<String>,
    /// loved · often · forgotten
    pub frequency: Vec<String>,
    /// workout · sleep · work · road · rest · party
    pub activity: Vec<String>,
    /// light · medium · intense
    pub workout_tempo: Option<String>,
    /// Daypart code 0…5 (manual atmosphere).
    pub daypart: Option<u8>,
    /// Weather class 0…5 (manual atmosphere).
    pub weather: Option<u8>,
    /// auto · winter · spring · summer · autumn · off
    pub season: Option<String>,
    /// Influence of season, context, events: 0…2 each.
    pub influence: Option<[f32; 3]>,
    pub energy: Option<[f32; 2]>,
    /// BPM.
    pub tempo: Option<[f32; 2]>,
    /// «Для всех» atmospheres (FR-ALL-003): background · company · party.
    pub gathering: Option<String>,
    /// Expert level (FR-LEARN-012): the taste's weight 0…2.
    pub taste: Option<f32>,
    /// Expert level: genres never in this wave.
    pub exclude_genres: Vec<String>,
    /// Expert level: one artist at most once in this many tracks (2…20).
    pub artist_spacing: Option<u32>,
}

fn keys<T>(v: &[String], parse: fn(&str) -> Option<T>) -> Result<Vec<T>, AppError> {
    v.iter()
        .map(|k| {
            parse(k)
                .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument).with_details(k.clone()))
        })
        .collect()
}

impl WaveQueryIpc {
    fn into_query(self) -> Result<WaveQuery, AppError> {
        use sirtone_core::wave::{Daypart, WeatherClass};
        let bad =
            |what: &str| AppError::new(ErrorCode::InvalidArgument).with_details(what.to_owned());
        let daypart = match self.daypart {
            None => None,
            Some(d) => Some(
                [
                    Daypart::Dawn,
                    Daypart::Morning,
                    Daypart::Day,
                    Daypart::Evening,
                    Daypart::Dusk,
                    Daypart::Night,
                ]
                .get(usize::from(d))
                .copied()
                .ok_or_else(|| bad("daypart"))?,
            ),
        };
        let weather = match self.weather {
            None => None,
            Some(w) => Some(
                [
                    WeatherClass::Clear,
                    WeatherClass::Cloudy,
                    WeatherClass::Fog,
                    WeatherClass::DrizzleRain,
                    WeatherClass::Snow,
                    WeatherClass::Storm,
                ]
                .get(usize::from(w))
                .copied()
                .ok_or_else(|| bad("weather"))?,
            ),
        };
        let season = match self.season.as_deref() {
            None | Some("auto") => SeasonPick::Auto,
            Some("off") => SeasonPick::Off,
            Some(s) => SeasonPick::Only(
                ["winter", "spring", "summer", "autumn"]
                    .iter()
                    .position(|k| *k == s)
                    .ok_or_else(|| bad("season"))? as u8,
            ),
        };
        if let Some(w) = self.workout_tempo.as_deref()
            && !["light", "medium", "intense"].contains(&w)
        {
            return Err(bad("workoutTempo"));
        }
        // «Для всех» atmospheres (§16) on the existing filters: «Фон» — calm energy, «Компания» —
        // middle energy and positive, «Вечеринка» — the party activity.
        let mut mood = self.mood.clone();
        let mut activity = self.activity.clone();
        let mut energy = self.energy;
        match self.gathering.as_deref() {
            None => {}
            Some("background") => energy = Some([0.0, 0.55]),
            Some("company") => {
                energy = Some([0.3, 0.75]);
                if !mood.iter().any(|m| m == "positive") {
                    mood.push("positive".into());
                }
            }
            Some("party") => {
                if !activity.iter().any(|a| a == "party") {
                    activity.push("party".into());
                }
            }
            Some(_) => return Err(bad("gathering")),
        }
        Ok(WaveQuery {
            mood: keys(&mood, Mood::from_key)?,
            character: keys(&self.character, Character::from_key)?,
            language: keys(&self.language, Lang::from_key)?,
            frequency: keys(&self.frequency, Freq::from_key)?,
            activity: keys(&activity, Activity::from_key)?,
            workout_tempo: self.workout_tempo,
            daypart,
            weather,
            season,
            influence: self.influence.unwrap_or([1.0; 3]),
            energy: energy.map(|[a, b]| (a, b)),
            tempo: self.tempo.map(|[a, b]| (a, b)),
            taste: self.taste.map_or(1.0, |v| v.clamp(0.0, 2.0)),
            exclude_genres: self
                .exclude_genres
                .iter()
                .take(64)
                .map(|g| g.trim().to_lowercase())
                .filter(|g| !g.is_empty())
                .collect(),
            artist_spacing: match self.artist_spacing {
                None => None,
                Some(n @ 2..=20) => Some(n as usize),
                Some(_) => return Err(bad("artistSpacing")),
            },
        })
    }
}

/// What `wave_start` did: tracks queued and the relaxation steps of «Настроить волну»
/// (not empty → «Мало треков — расширил подбор», FR-WAVE-006).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WaveStarted {
    pub queued: u32,
    /// mood · character · activity · tempo · energy · language · frequency
    pub relaxed: Vec<String>,
    /// «Для всех»: «подборка без данных о популярности» (FR-ALL-005).
    pub no_popularity: bool,
    /// «Для всех»: fewer than 30 suitable tracks — the honest caption.
    pub few: bool,
}

pub struct Wave {
    tx: Sender<Cmd>,
    /// A refill is on its way (the sink asks 4 times a second).
    pending: std::sync::Arc<AtomicBool>,
}

impl Wave {
    pub fn start(app: AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        let pending = std::sync::Arc::new(AtomicBool::new(false));
        let worker_pending = pending.clone();
        std::thread::Builder::new()
            .name("sirtone-wave".into())
            .spawn(move || worker(&app, &rx, &worker_pending))
            .ok();
        Self { tx, pending }
    }

    /// A wave track was skipped by the user (the player's «вперёд»).
    pub fn skipped(&self, track: u32) {
        let _ = self.tx.send(Cmd::Skipped(i64::from(track)));
    }

    /// Starts a wave with a built-in profile from Rust (focus, T4.24); plays at once.
    pub fn start_profile(&self, id: &str) {
        let (tx, _rx) = mpsc::channel();
        let _ = self
            .tx
            .send(Cmd::Start(id.to_owned(), WaveQuery::default(), tx));
    }

    /// The library's fit for an event (waits ≤ 10 s for the wave thread).
    pub fn event_fit(&self, id: &str) -> Option<sirtone_wave::events::Fit> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(Cmd::EventFit(id.to_owned(), tx)).ok()?;
        rx.recv_timeout(Duration::from_secs(10)).ok().flatten()
    }

    /// The wave goes on with another profile after the current track (focus work ↔ break):
    /// the upcoming wave tracks are dropped and picked again.
    pub fn switch_profile(&self, id: &str) {
        let _ = self.tx.send(Cmd::Switch(id.to_owned()));
    }

    /// More tracks for a wave that is running out (from the playback sink; never blocks).
    pub fn request_more(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(Cmd::More);
        }
    }
}

/// Slider position 0…1 from the settings (`wave.novelty`, 0–100).
fn novelty(app: &AppHandle) -> f32 {
    app.try_state::<std::sync::Arc<SettingsStore>>()
        .and_then(|s| s.get()["wave"]["novelty"].as_u64())
        .map_or(0.5, |v| (v.min(100) as f32) / 100.0)
}

/// «Занятие» by the foreground application (T5.6, FR-CTX-003): only when switched on in
/// Settings, only for «Моя волна» without a chosen activity; the exe name is read, matched
/// against `context.json` and dropped (never a window title, nothing stored). Sirtone itself in
/// front keeps the last answer.
fn detect_activity(
    app: &AppHandle,
    profile: &str,
    q: &WaveQuery,
    last: Option<Activity>,
) -> Option<Activity> {
    let on = app
        .try_state::<std::sync::Arc<SettingsStore>>()
        .is_some_and(|s| s.get()["context"]["activity_detection"].as_bool() == Some(true));
    if !on || profile != "my_wave" || !q.activity.is_empty() {
        return None;
    }
    let Some(exe) = sirtone_platform::sessions::foreground_exe() else {
        return last;
    };
    if exe == "sirtone-app.exe" || exe == "msedgewebview2.exe" {
        return last;
    }
    static CFG: std::sync::OnceLock<sirtone_wave::config::ContextConfig> =
        std::sync::OnceLock::new();
    CFG.get_or_init(sirtone_wave::config::ContextConfig::default)
        .activity_of(&exe)
        .and_then(Activity::from_key)
}

fn with_activity(q: &WaveQuery, a: Option<Activity>) -> WaveQuery {
    let mut q = q.clone();
    if let Some(a) = a {
        q.activity = vec![a];
    }
    q
}

/// «Непохожесть» of the expert level (FR-NOV-003); `None` — the main slider sets both.
fn distance(app: &AppHandle) -> Option<f32> {
    app.try_state::<std::sync::Arc<SettingsStore>>()
        .and_then(|s| s.get()["wave"]["novelty_distance"].as_u64())
        .map(|v| (v.min(100) as f32) / 100.0)
}

/// The wave thread's own connection to the current library: reading the catalog on a refill
/// must not hold the shared read connection the lists use (≤ 100 ms rule).
type Conn = Option<(std::path::PathBuf, rusqlite::Connection)>;

fn conn<'a>(app: &AppHandle, slot: &'a mut Conn) -> Result<&'a rusqlite::Connection, AppError> {
    let root = app
        .state::<Library>()
        .root()
        .ok_or_else(|| AppError::new(ErrorCode::LibraryRootNotSet))?;
    if slot.as_ref().is_none_or(|(r, _)| *r != root) {
        let c = sirtone_library::open_existing(
            &crate::library::db_path(&root),
            crate::library::medium(&root),
        )?;
        *slot = Some((root, c));
    }
    slot.as_ref()
        .map(|(_, c)| c)
        .ok_or_else(|| AppError::new(ErrorCode::Internal))
}

type Read = (
    Vec<sirtone_core::wave::WaveTrack>,
    Vec<sirtone_core::wave::NotNowRow>,
    History,
);

/// `wave_state` marks of learning (T3.28, docs/14 §8.4): «learn from» (reset) and «frozen at»
/// (learning switched off — the facts keep being written, the models stop).
const RESET_AT: &str = "learn_reset_at";
const FROZEN_AT: &str = "learn_frozen_at";

fn learning(app: &AppHandle) -> bool {
    app.try_state::<std::sync::Arc<SettingsStore>>()
        .and_then(|s| s.get()["wave"]["learning"].as_bool())
        .unwrap_or(true)
}

fn read(app: &AppHandle, slot: &mut Conn, now: i64) -> Result<Read, AppError> {
    use sirtone_library::wave as w;
    let learn = learning(app);
    let c = conn(app, slot)?;
    let mark = |k: &str| -> Result<Option<i64>, AppError> {
        Ok(w::state(c, k)?.and_then(|v| v.parse().ok()))
    };
    let from = mark(RESET_AT)?.unwrap_or(0);
    let frozen = match (learn, mark(FROZEN_AT)?) {
        (true, Some(_)) => {
            w::set_state(c, FROZEN_AT, None)?;
            None
        }
        (true, None) => None,
        (false, Some(t)) => Some(t),
        (false, None) => {
            w::set_state(c, FROZEN_AT, Some(&now.to_string()))?;
            Some(now)
        }
    };
    let until = frozen.unwrap_or(i64::MAX);
    let history = History {
        events: w::plays_since(c, from)?
            .into_iter()
            .filter(|e| e.ts < until)
            .collect(),
        feedback: w::feedback_since(c, from)?
            .into_iter()
            .filter(|f| f.ts < until)
            .collect(),
    };
    Ok((w::catalog(c)?, w::not_now(c, now)?, history))
}

/// The next `n` library tracks from the engine, built or refreshed from the library.
fn next_ids(
    app: &AppHandle,
    slot: &mut Conn,
    engine: &mut Option<Engine>,
    profile: &str,
    query: &WaveQuery,
    n: usize,
) -> Result<Vec<u32>, AppError> {
    let clock = SystemClock;
    let now = clock.now_utc_ms();
    let (tracks, shelf, history) = match read(app, slot, now) {
        Ok(v) => v,
        // A stale handle (the stick came back): reopen once.
        Err(_) => {
            *slot = None;
            read(app, slot, now)?
        }
    };
    match engine {
        Some(e) => e.reload(&tracks, &history, now),
        None => {
            let cfg = Config::builtin_profile(profile)
                .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e.to_string()))?;
            let mut e = Engine::new(cfg, &tracks, &history, now, now as u64 | 1);
            // The event wave (T5.13): the accepted event scores the tracks.
            if profile == "event" {
                let events = sirtone_wave::events::EventsConfig::default();
                let id = crate::events::accepted();
                e.set_event(id.as_deref().and_then(|i| events.get(i)), &tracks);
            }
            e.set_query(query.clone(), now);
            *engine = Some(e);
        }
    }
    let Some(e) = engine.as_mut() else {
        return Ok(Vec::new());
    };
    let ctx = crate::context::builder().snapshot(&clock);
    // The sleep wave (T4.22, §14): the timer's progress and E₀ — the energy of the track that
    // played when it started (taken once, at the first refill).
    if let Some(s) = app.try_state::<crate::sleep::Sleep>() {
        let state = s.fraction().map(|progress| {
            let e0 = s.e0().unwrap_or_else(|| {
                let id = app
                    .try_state::<crate::playback::Playback>()
                    .and_then(|p| p.current_track_id());
                let e0 = tracks
                    .iter()
                    .find(|t| id.is_some_and(|i| i64::from(i) == t.id))
                    .and_then(|t| t.features)
                    .map_or(0.5, |f| f.0[sirtone_wave::engine::f::ENERGY]);
                s.set_e0(e0);
                e0
            });
            sirtone_wave::engine::SleepState { e0, progress }
        });
        e.set_sleep(state);
    }
    // Focus (T4.24, §15): «Слова» and the segment to pack into (§7.1) — what is left of it when
    // the first new track would start: after the current one and the wave tracks already queued.
    if profile.starts_with("focus") {
        let doc = app
            .try_state::<std::sync::Arc<SettingsStore>>()
            .map(|s| s.get())
            .unwrap_or_default();
        e.set_vocals(match doc["focus"]["vocals"].as_str() {
            Some("none") => sirtone_wave::engine::Vocals::None,
            Some("any") => sirtone_wave::engine::Vocals::Any,
            _ => sirtone_wave::engine::Vocals::PreferNone,
        });
        let overrun = doc["focus"]["overrun_s"].as_i64().unwrap_or(90) * 1000;
        let pack = app
            .try_state::<crate::focus::Focus>()
            .and_then(|f| f.segment())
            .filter(|(p, _)| p == profile)
            .map(|(_, end)| {
                let left = end
                    .saturating_duration_since(std::time::Instant::now())
                    .as_millis() as i64;
                let pb = app.state::<Playback>();
                let s = pb.snapshot();
                let current = i64::from(s.duration_ms.unwrap_or(0).saturating_sub(s.position_ms));
                let queued: i64 = pb
                    .upcoming_ids()
                    .iter()
                    .filter_map(|id| tracks.iter().find(|t| t.id == i64::from(*id)))
                    .map(|t| t.duration_ms as i64)
                    .sum();
                sirtone_wave::engine::SegmentPack {
                    left_ms: left - current - queued,
                    overrun_ms: overrun,
                }
            });
        e.set_segment(pack);
    }
    e.set_distance(distance(app));
    Ok(e.next(&ctx, &shelf, novelty(app), n, now)
        .into_iter()
        .filter_map(|id| u32::try_from(id).ok())
        .collect())
}

fn worker(app: &AppHandle, rx: &Receiver<Cmd>, pending: &AtomicBool) {
    let mut engine: Option<Engine> = None;
    let mut slot: Conn = None;
    let mut profile = String::from("my_wave");
    let mut query = WaveQuery::default();
    // «Занятие» by the foreground application (T5.6), kept while Sirtone itself is in front.
    let mut auto: Option<Activity> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start(p, q, reply) => {
                // A new wave: a new session (spacing and group counts start over, every
                // influence is back on).
                engine = None;
                *OFF.lock().unwrap_or_else(|p| p.into_inner()) = WaveOff::default();
                profile = p;
                query = q;
                auto = detect_activity(app, &profile, &query, None);
                app.state::<Playback>().set_wave_profile(&profile);
                let eff = with_activity(&query, auto);
                let r =
                    next_ids(app, &mut slot, &mut engine, &profile, &eff, FIRST).and_then(|ids| {
                        let lib = app.state::<Library>();
                        let items = items_for(&lib, &ids, PlayContext::Wave)?;
                        let n = items.len() as u32;
                        if n > 0 {
                            app.state::<Playback>().play_queue(items, 0);
                        }
                        let (no_popularity, few) = engine
                            .as_ref()
                            .map_or((false, false), |e| e.company_captions());
                        Ok(WaveStarted {
                            queued: n,
                            relaxed: engine.as_ref().map_or_else(Vec::new, |e| {
                                e.relaxed().iter().map(|s| (*s).to_owned()).collect()
                            }),
                            no_popularity,
                            few,
                        })
                    });
                let _ = reply.send(r);
            }
            Cmd::Why(id, reply) => {
                let reasons = engine.as_ref().map_or_else(Vec::new, |e| {
                    e.why(id)
                        .into_iter()
                        .map(|w| {
                            use sirtone_wave::engine::Why;
                            let (kind, arg) = match w {
                                Why::Liked => ("liked", None),
                                Why::Genre(g) => ("genre", Some(g)),
                                Why::Taste => ("taste", None),
                                Why::Context(d) => ("context", Some(d)),
                                Why::Session => ("session", None),
                                Why::NewForYou => ("new", None),
                                Why::Forgotten => ("forgotten", None),
                                Why::Season(s) => ("season", Some(s)),
                                Why::ForAll => ("forAll", None),
                            };
                            WhyReason {
                                kind: kind.into(),
                                arg,
                            }
                        })
                        .collect()
                });
                let _ = reply.send(reasons);
            }
            Cmd::Skipped(id) => {
                if let Some(e) = engine.as_mut() {
                    e.skipped(id, SystemClock.now_utc_ms());
                    retune(app, &mut slot, &mut engine, &profile, &query);
                }
            }
            Cmd::Off => {
                let eff = off().apply(&with_activity(&query, auto));
                if let Some(e) = engine.as_mut() {
                    e.set_query(eff.clone(), SystemClock.now_utc_ms());
                    retune(app, &mut slot, &mut engine, &profile, &eff);
                }
            }
            Cmd::Count(q, reply) => {
                let now = SystemClock.now_utc_ms();
                let count = |e: &Engine| {
                    let (n, relaxed) = e.query_count(&q, now);
                    WaveCount {
                        tracks: n as u32,
                        relaxed,
                    }
                };
                let r = match &engine {
                    Some(e) => Some(count(e)),
                    None => read(app, &mut slot, now)
                        .ok()
                        .and_then(|(tracks, _, history)| {
                            let cfg = Config::builtin_profile("my_wave").ok()?;
                            Some(count(&Engine::new(cfg, &tracks, &history, now, 1)))
                        }),
                };
                let _ = reply.send(r);
            }
            Cmd::EventFit(id, reply) => {
                let now = SystemClock.now_utc_ms();
                let fit = read(app, &mut slot, now).ok().and_then(|(tracks, _, _)| {
                    let events = sirtone_wave::events::EventsConfig::default();
                    events.get(&id).map(|def| {
                        sirtone_wave::events::fit(&sirtone_wave::events::Matcher::new(def), &tracks)
                    })
                });
                let _ = reply.send(fit);
            }
            Cmd::Switch(p) => {
                engine = None;
                profile = p;
                query = WaveQuery::default();
                let pb = app.state::<Playback>();
                pb.set_wave_profile(&profile);
                pb.drop_upcoming_wave();
                match next_ids(app, &mut slot, &mut engine, &profile, &query, MORE)
                    .and_then(|ids| items_for(&app.state::<Library>(), &ids, PlayContext::Wave))
                {
                    Ok(items) if !items.is_empty() => pb.append(items),
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(target: "sirtone::wave", code = ?e.code, "switch failed");
                    }
                }
            }
            Cmd::Retune => {
                if engine.is_some() {
                    retune(app, &mut slot, &mut engine, &profile, &query);
                }
            }
            Cmd::More => {
                if engine.is_some() {
                    let now_auto = detect_activity(app, &profile, &query, auto);
                    if now_auto != auto {
                        auto = now_auto;
                        if let Some(e) = engine.as_mut() {
                            let eff = off().apply(&with_activity(&query, auto));
                            e.set_query(eff, SystemClock.now_utc_ms());
                        }
                    }
                    match next_ids(app, &mut slot, &mut engine, &profile, &query, MORE)
                        .and_then(|ids| items_for(&app.state::<Library>(), &ids, PlayContext::Wave))
                    {
                        Ok(items) if !items.is_empty() => app.state::<Playback>().append(items),
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!(target: "sirtone::wave", code = ?e.code, "refill failed");
                        }
                    }
                }
                pending.store(false, Ordering::Release);
            }
        }
    }
}

/// The upcoming wave tracks picked again (the slider moved, a wave track was skipped).
fn retune(
    app: &AppHandle,
    slot: &mut Conn,
    engine: &mut Option<Engine>,
    profile: &str,
    query: &WaveQuery,
) {
    let pb = app.state::<Playback>();
    let dropped = pb.drop_upcoming_wave();
    if dropped == 0 {
        return;
    }
    match next_ids(app, slot, engine, profile, query, dropped.max(MORE))
        .and_then(|ids| items_for(&app.state::<Library>(), &ids, PlayContext::Wave))
    {
        Ok(items) if !items.is_empty() => pb.append(items),
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(target: "sirtone::wave", code = ?e.code, "retune failed");
        }
    }
}

/// «Моя волна» or another built-in profile (`seasonal` — «Сезонная волна», FR-SEAS-003) with
/// optional «Настроить волну» filters: the engine picks and plays.
#[tauri::command(async)]
#[specta::specta]
pub fn wave_start(
    wave: State<'_, Wave>,
    profile: Option<String>,
    query: Option<WaveQueryIpc>,
) -> Result<WaveStarted, AppError> {
    let profile = match profile.as_deref() {
        Some("seasonal") => "seasonal",
        Some("for_all") => "for_all",
        Some("sleep") => "sleep",
        Some("event") => "event",
        _ => "my_wave",
    };
    let query = query.unwrap_or_default().into_query()?;
    let (tx, rx) = mpsc::channel();
    wave.tx
        .send(Cmd::Start(profile.into(), query, tx))
        .map_err(|_| AppError::new(ErrorCode::Internal))?;
    rx.recv_timeout(Duration::from_secs(20))
        .map_err(|_| AppError::new(ErrorCode::Internal).with_details("wave start timed out"))?
}

/// «Подходит N треков» (T5.7, `06` §3): the live counter of «Настроить волну».
#[derive(Debug, Clone, Copy, serde::Serialize, specta::Type)]
pub struct WaveCount {
    pub tracks: u32,
    /// Fewer than enough — the wave will widen the filters («Расширил подбор»).
    pub relaxed: bool,
}

#[tauri::command(async)]
#[specta::specta]
pub fn wave_query_count(
    wave: State<'_, Wave>,
    query: WaveQueryIpc,
) -> Result<Option<WaveCount>, AppError> {
    let query = query.into_query()?;
    let (tx, rx) = mpsc::channel();
    wave.tx
        .send(Cmd::Count(query, tx))
        .map_err(|_| AppError::new(ErrorCode::Internal))?;
    Ok(rx.recv_timeout(Duration::from_secs(10)).ok().flatten())
}

/// «Сбросить обучение» (docs/14 §8.4): the models start over from now; the journal stays.
#[tauri::command(async)]
#[specta::specta]
pub fn wave_reset_learning(lib: State<'_, Library>) -> Result<(), AppError> {
    let now = SystemClock.now_utc_ms();
    lib.with_write(|c| sirtone_library::wave::set_state(c, RESET_AT, Some(&now.to_string())))
}

/// «Почему этот трек» for a track of the running wave (empty outside the wave).
#[tauri::command(async)]
#[specta::specta]
pub fn wave_why(wave: State<'_, Wave>, track: u32) -> Vec<WhyReason> {
    let (tx, rx) = mpsc::channel();
    if wave.tx.send(Cmd::Why(i64::from(track), tx)).is_err() {
        return Vec::new();
    }
    rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default()
}

/// «Знакомое ↔ Неожиданное» moved (docs/14 §9): the upcoming wave tracks follow at once.
#[tauri::command]
#[specta::specta]
pub fn wave_retune(wave: State<'_, Wave>) {
    let _ = wave.tx.send(Cmd::Retune);
}

/// The seasonal card's name (FR-SEAS-004, docs/14 §10.3) as an i18n key under `wave.season`:
/// the season or, in a transition window, the mix («Предзимье»).
#[tauri::command]
#[specta::specta]
pub fn wave_season_card() -> String {
    let ctx = crate::context::builder().snapshot(&SystemClock);
    sirtone_wave::season::card_key(&ctx.season_w).to_owned()
}

/// The seasonal/event theme of today (FR-THEME-001, docs/14 §12): the accent seed (the UI's
/// generator makes the 6 tokens with their contrast), the visualiser palette, the cover
/// placeholder variant and the linked effect. Same date path as `wave_season_card`, so the
/// colour and the card name never disagree. Events join with T5.14.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ThemeStateDto {
    pub id: String,
    pub card: String,
    /// `season` | `event`
    pub source: String,
    pub seed: String,
    pub viz: Vec<String>,
    pub cover_variant: String,
    pub effect: String,
}

#[tauri::command]
#[specta::specta]
pub fn theme_state() -> ThemeStateDto {
    use sirtone_wave::theme::{ThemeSource, ThemesConfig, resolve};
    let ctx = crate::context::builder().snapshot(&SystemClock);
    // An accepted event inside its window brings its theme (Q-3), then the season again.
    let event = crate::events::accepted();
    let s = resolve(&ThemesConfig::default(), &ctx.season_w, event.as_deref());
    ThemeStateDto {
        id: s.id,
        card: s.card,
        source: match s.source {
            ThemeSource::Season => "season",
            ThemeSource::Event => "event",
        }
        .to_owned(),
        seed: s.seed,
        viz: s.viz,
        cover_variant: s.cover_variant,
        effect: s.effect,
    }
}

/// The live effects' sources right now (T4.20, T5.15, `14` §13.1, supplement §31): «По сезону»
/// (the season's, the summer night's fireflies by the daypart — by the sun where the city is
/// known) and «По реальной погоде» — `None` without fresh weather (then the season's, AT-16), plus
/// the wind's drift and the weather's intensity for the particles.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FxSources {
    pub season: String,
    pub weather: Option<String>,
    /// The accepted event's effect (fog for Halloween, AT-13): above the weather and the season.
    pub event: Option<String>,
    /// −1…1, + — to the right.
    pub drift: f32,
    /// 0…1.
    pub intensity: f32,
}

#[tauri::command]
#[specta::specta]
pub fn fx_sources() -> FxSources {
    use sirtone_wave::theme::{ThemesConfig, season_effect, weather_effect};
    let ctx = crate::context::builder().snapshot(&SystemClock);
    let daypart = daypart_id(ctx.daypart);
    let cfg = ThemesConfig::default();
    let w = crate::context::weather_now();
    let night = cfg.night_dayparts.iter().any(|d| d == daypart);
    FxSources {
        season: season_effect(&cfg, &ctx.season_w, daypart),
        weather: w
            .class
            .as_deref()
            .and_then(|c| weather_effect(&cfg, &ctx.season_w, c, w.is_day && !night)),
        event: crate::events::accepted_effect(),
        drift: w.drift,
        intensity: w.intensity,
    }
}

fn daypart_id(d: sirtone_core::wave::Daypart) -> &'static str {
    use sirtone_core::wave::Daypart;
    match d {
        Daypart::Dawn => "dawn",
        Daypart::Morning => "morning",
        Daypart::Day => "day",
        Daypart::Evening => "evening",
        Daypart::Dusk => "dusk",
        Daypart::Night => "night",
    }
}

/// The contextual name of «Моя волна» (T5.11, FR-WAVE-007): «Дождливый вечер», «Зимнее утро»
/// — the weather (fresh only) or else the season, and the part of the day (by the sun where the
/// city is known), plus when the part of the day changes next. The UI builds the words.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WaveContextName {
    pub daypart: String,
    /// `clear` · `cloudy` · `fog` · `rain` · `snow` · `storm`; `None` — no fresh weather.
    pub weather: Option<String>,
    /// The dominant season: `winter` · `spring` · `summer` · `autumn`.
    pub season: String,
    /// UTC ms of the next change of the part of the day (the name changes then).
    pub next_at: f64,
}

#[tauri::command]
#[specta::specta]
pub fn wave_context_name() -> WaveContextName {
    let b = crate::context::builder();
    let clock = SystemClock;
    let now = clock.now_utc_ms();
    let ctx = b.snapshot_at(&clock, now);
    // The next boundary, to 5 minutes (≤ 26 h ahead).
    const STEP: i64 = 5 * 60_000;
    let next_at = (1..=26 * 12)
        .map(|i| now - now % STEP + i * STEP)
        .find(|&t| b.snapshot_at(&clock, t).daypart != ctx.daypart)
        .unwrap_or(now + 26 * 3_600_000);
    let season = ctx
        .season_w
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(i, _)| i);
    WaveContextName {
        daypart: daypart_id(ctx.daypart).to_owned(),
        weather: crate::context::weather_now().class,
        season: ["winter", "spring", "summer", "autumn"][season].to_owned(),
        next_at: next_at as f64,
    }
}

/// «Состав волны» (T5.16, FR-LEARN-010, `14` §17; `01c` W-02 — the YouTube Music tuner): which
/// influences the session has switched off (weight → 0, the rest renormalised).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct WaveOff {
    pub season: bool,
    /// Time of day and weather.
    pub context: bool,
    pub event: bool,
    pub taste: bool,
}

impl WaveOff {
    fn apply(&self, q: &WaveQuery) -> WaveQuery {
        let mut q = q.clone();
        for (i, off) in [self.season, self.context, self.event]
            .into_iter()
            .enumerate()
        {
            if off {
                q.influence[i] = 0.0;
            }
        }
        if self.taste {
            q.taste = 0.0;
        }
        q
    }
}

static OFF: std::sync::Mutex<WaveOff> = std::sync::Mutex::new(WaveOff {
    season: false,
    context: false,
    event: false,
    taste: false,
});

fn off() -> WaveOff {
    *OFF.lock().unwrap_or_else(|p| p.into_inner())
}

/// The chips under the player: what steers the wave now.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WaveComposition {
    pub season: String,
    pub daypart: String,
    /// `None` — no fresh weather (no chip).
    pub weather: Option<String>,
    /// The event of the event wave and its days to the peak.
    pub event: Option<String>,
    pub event_days: i32,
    pub off: WaveOff,
}

#[tauri::command]
#[specta::specta]
pub fn wave_composition() -> WaveComposition {
    let c = wave_context_name();
    let ev = crate::events::accepted_days();
    WaveComposition {
        season: c.season,
        daypart: c.daypart,
        weather: c.weather,
        event_days: ev.as_ref().map_or(0, |e| e.1),
        event: ev.map(|e| e.0),
        off: off(),
    }
}

/// A chip pressed: the influence off (or back on) for this session; the upcoming tracks are
/// picked again.
#[tauri::command]
#[specta::specta]
pub fn wave_set_off(wave: State<'_, Wave>, off: WaveOff) {
    *OFF.lock().unwrap_or_else(|p| p.into_inner()) = off;
    let _ = wave.tx.send(Cmd::Off);
}
