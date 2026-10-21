//! Event waves (T5.13–T5.14, FR-EVT-001…004, THEME-003, `14` §11, D-109): once a minute the
//! offline calendar is read at the local date (time only from `Clock`) with the weather's
//! micro-event triggers; at most one card a day for the whole app offers an event that the
//! library has enough music for. «Включить» starts the event wave (profile `event`) and — with
//! «Тема по сезону» on — the event's theme and effect until the window ends; «Позже» asks again
//! tomorrow, «Не в этом году», «Никогда»; three declines in a row switch the event off. The auto
//! mode starts it without a card (never 9 May). Per-event switches live in settings `events`;
//! the per-year state in `events-state.json` in the data folder (not the library, D-109).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::wave::{Clock, WeatherClass};
use sirtone_platform::clock::SystemClock;
use sirtone_wave::clock::local_time;
use sirtone_wave::events::{Active, EventDef, EventsConfig, WeatherFlags, When, active};
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::settings_store::SettingsStore;

const TICK: Duration = Duration::from_secs(60);

/// The card on «Моя волна» and the tray line (FR-EVT-004).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct EventOffer {
    /// `None` — no card now.
    pub id: Option<String>,
    pub days_to_peak: i32,
    /// `high` on the peak day of a strong event — the card stands out.
    pub strong: bool,
    /// Few direct matches — «волна в … настроении» (the honest caption, AT-23).
    pub atmospheric: bool,
}

impl EventOffer {
    fn none() -> Self {
        Self {
            id: None,
            days_to_peak: 0,
            strong: false,
            atmospheric: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct EventYear {
    year: i32,
    /// `shown` · `accepted` · `later` · `declined_year`
    state: String,
    last_shown_day: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StateFile {
    /// The day the last card was shown (≤ 1 a day for the whole app).
    last_card_day: i32,
    events: BTreeMap<String, EventYear>,
    /// Declines in a row (Позже / closing the card) per event; «Включить» resets it.
    declines: BTreeMap<String, u32>,
    /// «Первый снег» once a season: the season key (`2026-winter`) and the day it happened.
    first_snow: Option<(String, i32)>,
    /// «Первый тёплый день» once a year: the year and the day.
    first_warm: Option<(i32, i32)>,
}

struct Shared {
    data: PathBuf,
    state: StateFile,
    offer: EventOffer,
    /// The accepted event inside its window — the theme, the effect, the journal.
    accepted: Option<String>,
    /// Its days to the peak (the chip «Хэллоуин через 3 дня»).
    accepted_days: i32,
    /// Fit per event for today: (day, direct, by tone).
    fit_cache: BTreeMap<String, (i32, usize, usize)>,
}

static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();
static KICK: OnceLock<Mutex<std::sync::mpsc::Sender<()>>> = OnceLock::new();

fn cfg() -> &'static EventsConfig {
    static C: OnceLock<EventsConfig> = OnceLock::new();
    C.get_or_init(EventsConfig::default)
}

fn file(data: &std::path::Path) -> PathBuf {
    data.join("events-state.json")
}

fn save(s: &Shared) {
    if let Ok(b) = serde_json::to_vec_pretty(&s.state) {
        let _ = std::fs::write(file(&s.data), b);
    }
}

/// The accepted event inside its window (theme, effect, journal).
pub fn accepted() -> Option<String> {
    SHARED.get()?.lock().ok()?.accepted.clone()
}

/// The accepted event and its days to the peak («Состав волны»).
pub fn accepted_days() -> Option<(String, i32)> {
    let s = SHARED.get()?.lock().ok()?;
    s.accepted.clone().map(|id| (id, s.accepted_days))
}

/// Its effect (`fog`, `snow`), if it has one.
pub fn accepted_effect() -> Option<String> {
    accepted().and_then(|id| cfg().get(&id).and_then(|e| e.effect.clone()))
}

pub fn offer() -> EventOffer {
    SHARED
        .get()
        .and_then(|s| s.lock().ok().map(|s| s.offer.clone()))
        .unwrap_or_else(EventOffer::none)
}

/// Re-read now (settings, weather, a new day).
pub fn kick() {
    if let Some(tx) = KICK.get()
        && let Ok(tx) = tx.lock()
    {
        let _ = tx.send(());
    }
}

fn settings(app: &AppHandle) -> serde_json::Value {
    app.try_state::<Arc<SettingsStore>>()
        .map(|s| s.get())
        .unwrap_or_default()
}

/// Is the event switched on (its own switch, else its default; groups: religious, micro).
fn enabled(doc: &serde_json::Value, e: &EventDef) -> bool {
    let ev = &doc["events"];
    if let Some(on) = ev["enabled"][&e.id].as_bool() {
        return on;
    }
    if e.group == "religious" && ev["religious"].as_bool() == Some(false) {
        return false;
    }
    if let Some(g) = e.group.strip_prefix("micro_")
        && ev["micro"][g].as_bool() == Some(false)
    {
        return false;
    }
    e.default_on
}

fn season_key(day: i32) -> String {
    let (y, m, _) = sirtone_wave::calendar::civil_from_days(day);
    let s = match m {
        12 | 1 | 2 => "winter",
        3..=5 => "spring",
        6..=8 => "summer",
        _ => "autumn",
    };
    // A winter belongs to the year it began.
    let y = if m <= 2 { y - 1 } else { y };
    format!("{y}-{s}")
}

/// Tests may skip the «≥ 30 tracks» gate (a tiny fixture library): only with SIRTONE_DATA_DIR.
fn gate_bypass() -> bool {
    std::env::var_os("SIRTONE_DATA_DIR").is_some()
        && std::env::var_os("SIRTONE_SMOKE_EVENTS_ANY").is_some()
}

fn tick(app: &AppHandle) {
    let clock = SystemClock;
    let now = clock.now_utc_ms();
    let t = local_time(&clock, now);
    let builder = crate::context::builder();
    let w = builder.weather.as_ref().filter(|w| w.fresh(now));
    let doc = settings(app);
    let Some(sh) = SHARED.get() else { return };
    // The weather's micro-event triggers, once per season / year.
    let flags = {
        let Ok(mut s) = sh.lock() else { return };
        let key = season_key(t.day);
        if w.and_then(sirtone_wave::weather::WeatherState::class) == Some(WeatherClass::Snow)
            && s.state.first_snow.as_ref().is_none_or(|(k, _)| *k != key)
        {
            s.state.first_snow = Some((key.clone(), t.day));
            save(&s);
        }
        let (y, m, _) = sirtone_wave::calendar::civil_from_days(t.day);
        if (3..=5).contains(&m)
            && w.is_some_and(|w| w.temp_c >= 15.0)
            && s.state.first_warm.is_none_or(|(yy, _)| yy != y)
        {
            s.state.first_warm = Some((y, t.day));
            save(&s);
        }
        WeatherFlags {
            first_snow: s
                .state
                .first_snow
                .as_ref()
                .is_some_and(|(k, d)| *k == key && *d == t.day),
            first_warm: s.state.first_warm.is_some_and(|(_, d)| d == t.day),
            storm: w.and_then(sirtone_wave::weather::WeatherState::class)
                == Some(WeatherClass::Storm),
        }
    };
    let when = When {
        day: t.day,
        minute: t.minute,
        offset_min: clock.local_offset_min(now),
        southern: builder.southern,
        weather: flags,
    };
    let list: Vec<Active> = active(cfg(), &when)
        .into_iter()
        .filter(|a| cfg().get(&a.id).is_some_and(|e| enabled(&doc, e)))
        .collect();
    // The accepted event still inside its window.
    let accepted = {
        let Ok(s) = sh.lock() else { return };
        list.iter()
            .find(|a| {
                s.state
                    .events
                    .get(&a.id)
                    .is_some_and(|y| y.year == a.anchor_year && y.state == "accepted")
            })
            .map(|a| (a.id.clone(), a.days_to_peak))
    };
    let accepted_days = accepted.as_ref().map_or(0, |a| a.1);
    let accepted = accepted.map(|a| a.0);
    let changed_accepted = sh.lock().ok().is_some_and(|mut s| {
        s.accepted_days = accepted_days;
        std::mem::replace(&mut s.accepted, accepted.clone()) != accepted
    });
    if changed_accepted {
        // The theme and the effect follow (back to the season after the window, AT-13).
        let _ = offer().emit(app);
    }
    // The card: ≤ 1 a day, the strongest eligible event.
    let suggest = doc["events"]["suggest"].as_bool().unwrap_or(true);
    let auto = doc["events"]["auto_play"].as_bool().unwrap_or(false);
    let today_done = sh.lock().is_ok_and(|s| s.state.last_card_day == t.day);
    if accepted.is_some() || today_done || (!suggest && !auto) {
        return;
    }
    for a in &list {
        let Some(def) = cfg().get(&a.id) else {
            continue;
        };
        let open = sh.lock().is_ok_and(|s| match s.state.events.get(&a.id) {
            Some(y) if y.year == a.anchor_year => {
                y.state == "later" && y.last_shown_day < t.day
                    || y.state == "shown" && y.last_shown_day < t.day
            }
            _ => true,
        });
        if !open {
            continue;
        }
        let Some((direct, tone)) = fit_today(app, def, t.day) else {
            continue;
        };
        let fit = sirtone_wave::events::Fit { direct, tone };
        if !fit.offerable() && !gate_bypass() {
            continue;
        }
        if auto && def.auto {
            accept(app, &a.id, a.anchor_year, t.day);
            return;
        }
        if !suggest {
            continue;
        }
        let o = EventOffer {
            id: Some(a.id.clone()),
            days_to_peak: a.days_to_peak,
            strong: def.suggest_strength == "high" && a.days_to_peak == 0,
            atmospheric: fit.atmospheric(),
        };
        if let Ok(mut s) = sh.lock() {
            s.state.last_card_day = t.day;
            s.state.events.insert(
                a.id.clone(),
                EventYear {
                    year: a.anchor_year,
                    state: "shown".into(),
                    last_shown_day: t.day,
                },
            );
            s.offer = o.clone();
            save(&s);
        }
        let _ = o.emit(app);
        crate::tray::refresh_modes(app);
        return;
    }
}

/// The library's fit for the event today (cached per day; asked from the wave thread).
fn fit_today(app: &AppHandle, def: &EventDef, day: i32) -> Option<(usize, usize)> {
    if let Some(&(d, a, b)) = SHARED.get()?.lock().ok()?.fit_cache.get(&def.id)
        && d == day
    {
        return Some((a, b));
    }
    let f = app.try_state::<crate::wave::Wave>()?.event_fit(&def.id)?;
    if let Ok(mut s) = SHARED.get()?.lock() {
        s.fit_cache.insert(def.id.clone(), (day, f.direct, f.tone));
    }
    Some((f.direct, f.tone))
}

fn accept(app: &AppHandle, id: &str, year: i32, day: i32) {
    if let Some(sh) = SHARED.get()
        && let Ok(mut s) = sh.lock()
    {
        s.state.events.insert(
            id.to_owned(),
            EventYear {
                year,
                state: "accepted".into(),
                last_shown_day: day,
            },
        );
        s.state.declines.remove(id);
        s.accepted = Some(id.to_owned());
        s.offer = EventOffer::none();
        save(&s);
    }
    if let Some(w) = app.try_state::<crate::wave::Wave>() {
        w.start_profile("event");
    }
    kick(); // the days to the peak for the chip
    let _ = offer().emit(app);
    crate::tray::refresh_modes(app);
}

pub fn start(app: &AppHandle, data: PathBuf) {
    let state: StateFile = std::fs::read(file(&data))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let _ = SHARED.set(Mutex::new(Shared {
        data,
        state,
        offer: EventOffer::none(),
        accepted: None,
        accepted_days: 0,
        fit_cache: BTreeMap::new(),
    }));
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _ = KICK.set(Mutex::new(tx));
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("sirtone-events".into())
        .spawn(move || {
            // After the start settles (the library, the weather).
            let _ = rx.recv_timeout(Duration::from_secs(8));
            loop {
                tick(&app);
                match rx.recv_timeout(TICK) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        });
}

/// The card now.
#[tauri::command]
#[specta::specta]
pub fn event_offer() -> EventOffer {
    offer()
}

/// The answer to the card: `accept` · `later` · `year` · `never`.
#[tauri::command]
#[specta::specta]
pub fn event_answer(app: AppHandle, id: String, answer: String) -> Result<(), AppError> {
    let clock = SystemClock;
    let day = local_time(&clock, clock.now_utc_ms()).day;
    let def = cfg()
        .get(&id)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument).with_details(id.clone()))?;
    let year = {
        let sh = SHARED
            .get()
            .ok_or_else(|| AppError::new(ErrorCode::Internal))?;
        let s = sh.lock().map_err(|_| AppError::new(ErrorCode::Internal))?;
        s.state.events.get(&id).map_or_else(
            || sirtone_wave::calendar::civil_from_days(day).0,
            |y| y.year,
        )
    };
    match answer.as_str() {
        "accept" => {
            accept(&app, &def.id, year, day);
            return Ok(());
        }
        "later" | "year" | "never" => {}
        _ => {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_details(answer));
        }
    }
    let mut switch_off: Option<&str> = None;
    if let Some(sh) = SHARED.get()
        && let Ok(mut s) = sh.lock()
    {
        let state = if answer == "year" {
            "declined_year"
        } else {
            "later"
        };
        s.state.events.insert(
            id.clone(),
            EventYear {
                year,
                state: state.into(),
                last_shown_day: day,
            },
        );
        let n = s.state.declines.entry(id.clone()).or_insert(0);
        *n += 1;
        if answer == "never" {
            switch_off = Some("forever");
        } else if *n >= 3 {
            // Three declines in a row — off, with a mark (FR-EVT-004).
            switch_off = Some("declined_3x");
        }
        s.offer = EventOffer::none();
        save(&s);
    }
    if let Some(reason) = switch_off
        && let Some(st) = app.try_state::<Arc<SettingsStore>>()
    {
        let _ = st.patch(&serde_json::json!({
            "events": { "enabled": { &id: false }, "off_reason": { &id: reason } }
        }));
    }
    let _ = offer().emit(&app);
    crate::tray::refresh_modes(&app);
    Ok(())
}

/// One event for the settings list.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EventInfo {
    pub id: String,
    pub group: String,
    pub enabled: bool,
    /// `declined_3x` · `forever` — why it is off, if it was switched off for the person.
    pub off_reason: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn event_list(app: AppHandle) -> Vec<EventInfo> {
    let doc = settings(&app);
    cfg()
        .events
        .iter()
        .map(|e| EventInfo {
            id: e.id.clone(),
            group: e.group.clone(),
            enabled: enabled(&doc, e),
            off_reason: doc["events"]["off_reason"][&e.id]
                .as_str()
                .map(str::to_owned),
        })
        .collect()
}

/// For the settings screen: re-read after a switch.
#[tauri::command]
#[specta::specta]
pub fn event_refresh(_state: State<'_, Arc<SettingsStore>>) {
    kick();
}
