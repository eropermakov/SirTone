//! The context of the moment (T5.5, FR-CTX-001/002/004, `05` §10.2, `14` §3.2): the chosen city
//! (settings `context.city`), the daypart by the sun there, the hemisphere, and the weather from
//! Open-Meteo — fetched in the background every 30 min while the window is shown, 60 min while
//! hidden, kept in `weather.json` in the data folder (D-108), «stale» after 3 h. Without a city or
//! a network the context is the clock's (AT-16). Every snapshot in the app comes from `builder()`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::wave::Clock;
use sirtone_net::Host;
use sirtone_platform::clock::SystemClock;
use sirtone_wave::context::ContextBuilder;
use sirtone_wave::weather::{self, WeatherState};
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::net::NetState;
use crate::settings_store::SettingsStore;

/// The weather fetch rhythm (FR-CTX-002).
const SHOWN_EVERY_MS: i64 = 30 * 60_000;
const HIDDEN_EVERY_MS: i64 = 60 * 60_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct City {
    pub name: String,
    pub country: Option<String>,
    pub admin: Option<String>,
    pub lat: f64,
    pub lon: f64,
}

/// The weather as the UI shows it (and the effects use).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct WeatherNow {
    /// `clear` · `cloudy` · `fog` · `rain` · `snow` · `storm`; `None` — no fresh weather.
    pub class: Option<String>,
    pub temp_c: Option<f32>,
    pub is_day: bool,
    /// −1…1: where the wind pushes particles (+ right).
    pub drift: f32,
    pub intensity: f32,
    /// Unix ms of the fetch; `None` — never.
    pub fetched_at: Option<f64>,
    /// Younger than 3 h.
    pub fresh: bool,
}

struct State {
    builder: ContextBuilder,
    data: PathBuf,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

/// The context builder as of now (the city, the weather); before the app set it — the clock's.
pub fn builder() -> ContextBuilder {
    STATE
        .get()
        .and_then(|s| s.lock().ok().map(|s| s.builder.clone()))
        .unwrap_or_default()
}

fn weather_file(data: &std::path::Path) -> PathBuf {
    data.join("weather.json")
}

fn city_of(app: &AppHandle) -> Option<City> {
    let doc = app.try_state::<Arc<SettingsStore>>()?.get();
    serde_json::from_value(doc["context"]["city"].clone()).ok()
}

fn class_key(c: sirtone_core::wave::WeatherClass) -> &'static str {
    use sirtone_core::wave::WeatherClass as W;
    match c {
        W::Clear => "clear",
        W::Cloudy => "cloudy",
        W::Fog => "fog",
        W::DrizzleRain => "rain",
        W::Snow => "snow",
        W::Storm => "storm",
    }
}

/// The weather for the UI and the effects (`class: None` — none or stale).
pub fn weather_now() -> WeatherNow {
    let now = SystemClock.now_utc_ms();
    let w = builder().weather;
    let fresh = w.as_ref().is_some_and(|w| w.fresh(now));
    WeatherNow {
        class: w
            .as_ref()
            .filter(|_| fresh)
            .and_then(WeatherState::class)
            .map(|c| class_key(c).to_owned()),
        temp_c: w.as_ref().filter(|_| fresh).map(|w| w.temp_c),
        is_day: w.as_ref().is_none_or(|w| w.is_day),
        drift: w
            .as_ref()
            .filter(|_| fresh)
            .map_or(0.0, WeatherState::drift),
        intensity: w.as_ref().filter(|_| fresh).map_or(0.0, |w| w.intensity),
        fetched_at: w.as_ref().map(|w| w.fetched_at_ms as f64),
        fresh,
    }
}

/// The settings or the weather changed: rebuild the shared builder.
fn rebuild(app: &AppHandle) {
    let Some(st) = STATE.get() else { return };
    let city = city_of(app);
    if let Ok(mut s) = st.lock() {
        s.builder.coords = city.as_ref().map(|c| (c.lat, c.lon));
        s.builder.southern = city.as_ref().is_some_and(|c| c.lat < 0.0);
        // Weather for another place is not this place's.
        if let (Some(w), Some(c)) = (&s.builder.weather, &city)
            && ((w.lat - c.lat).abs() > 0.05 || (w.lon - c.lon).abs() > 0.05)
        {
            s.builder.weather = None;
        }
        if city.is_none() {
            s.builder.weather = None;
        }
    }
}

/// The settings changed (`context`): the city applies now and the weather is fetched again.
pub fn apply(app: &AppHandle) {
    rebuild(app);
    if let Some(tx) = KICK.get()
        && let Ok(tx) = tx.lock()
    {
        let _ = tx.send(());
    }
}

static KICK: OnceLock<Mutex<std::sync::mpsc::Sender<()>>> = OnceLock::new();

fn fetch(app: &AppHandle, city: &City) -> Option<WeatherState> {
    let net = app.try_state::<NetState>()?;
    let body = net
        .0
        .get(
            Host::OpenMeteo,
            &weather::forecast_query(city.lat, city.lon),
        )
        .ok()?;
    weather::parse(&body, city.lat, city.lon, SystemClock.now_utc_ms())
}

fn window_shown(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// Starts the context: the weather saved last time, then the background fetches.
pub fn start(app: &AppHandle, data: PathBuf) {
    let saved: Option<WeatherState> = std::fs::read(weather_file(&data))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let _ = STATE.set(Mutex::new(State {
        builder: ContextBuilder {
            weather: saved,
            ..ContextBuilder::default()
        },
        data,
    }));
    rebuild(app);
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let _ = KICK.set(Mutex::new(tx));
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("sirtone-weather".into())
        .spawn(move || {
            // A little after the start: the window and the network settle first.
            let _ = rx.recv_timeout(Duration::from_secs(20));
            loop {
                if let Some(city) = city_of(&app) {
                    let now = SystemClock.now_utc_ms();
                    let last = builder().weather.map_or(0, |w| w.fetched_at_ms);
                    let every = if window_shown(&app) {
                        SHOWN_EVERY_MS
                    } else {
                        HIDDEN_EVERY_MS
                    };
                    if now - last >= every
                        && let Some(w) = fetch(&app, &city)
                    {
                        if let Some(st) = STATE.get()
                            && let Ok(mut s) = st.lock()
                        {
                            if let Ok(b) = serde_json::to_vec(&w) {
                                let _ = std::fs::write(weather_file(&s.data), b);
                            }
                            s.builder.weather = Some(w);
                        }
                        let _ = weather_now().emit(&app);
                    }
                }
                // A settings change kicks it; otherwise look again in a minute.
                match rx.recv_timeout(Duration::from_secs(60)) {
                    Ok(()) => {
                        // A new city: the old weather is gone, fetch now.
                        if let Some(st) = STATE.get()
                            && let Ok(mut s) = st.lock()
                            && let (Some(w), Some(c)) = (&s.builder.weather, city_of(&app))
                            && ((w.lat - c.lat).abs() > 0.05 || (w.lon - c.lon).abs() > 0.05)
                        {
                            s.builder.weather = None;
                        }
                        let _ = weather_now().emit(&app);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        });
}

#[derive(Deserialize)]
struct GeoAnswer {
    #[serde(default)]
    results: Vec<GeoHit>,
}

#[derive(Deserialize)]
struct GeoHit {
    name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    admin1: Option<String>,
}

/// Cities by name (Open-Meteo geocoder, in Russian); offline — an error the UI words.
#[tauri::command(async)]
#[specta::specta]
pub fn city_search(app: AppHandle, query: String) -> Result<Vec<City>, AppError> {
    let q = query.trim();
    if q.chars().count() < 2 {
        return Ok(Vec::new());
    }
    let net = app
        .try_state::<NetState>()
        .ok_or_else(|| AppError::new(ErrorCode::Internal))?;
    let path = format!(
        "search?name={}&count=8&language=ru&format=json",
        sirtone_net::encode(q)
    );
    let body = net
        .0
        .get(Host::Geocoding, &path)
        .map_err(|_| AppError::new(ErrorCode::NetworkOffline))?;
    let a: GeoAnswer = serde_json::from_slice(&body)
        .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e.to_string()))?;
    Ok(a.results
        .into_iter()
        .map(|h| City {
            name: h.name,
            country: h.country,
            admin: h.admin1,
            lat: h.latitude,
            lon: h.longitude,
        })
        .collect())
}

/// The weather now (UI line, effects).
#[tauri::command]
#[specta::specta]
pub fn weather_get() -> WeatherNow {
    weather_now()
}

/// «Определить автоматически» (D-54): the Windows location service, coarse, only on this press;
/// denied or off — an error the UI words («включите определение местоположения в Windows»).
#[tauri::command(async)]
#[specta::specta]
pub fn city_locate() -> Result<City, AppError> {
    let (lat, lon) = sirtone_platform::geo::locate()
        .map_err(|e| AppError::new(ErrorCode::PermissionDenied).with_details(e))?;
    Ok(City {
        name: String::new(),
        country: None,
        admin: None,
        lat,
        lon,
    })
}
