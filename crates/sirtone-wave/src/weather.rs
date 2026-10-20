//! The weather (T5.5, FR-CTX-002, `14` §3.3, supplement §31): Open-Meteo's `current` block
//! normalised into a `WeatherState` — the WMO class, intensity, wind (speed and the direction it
//! blows from), `is_day`, temperature — plus the day's sunrise and sunset. Pure: the app fetches.
//! Older than 3 h — stale (`fresh`), then nothing uses it (AT-16).

use serde::{Deserialize, Serialize};
use sirtone_core::wave::WeatherClass;

/// The request's query for a point (coordinates rounded to 0.01°, `08` §4 — privacy).
pub fn forecast_query(lat: f64, lon: f64) -> String {
    format!(
        "forecast?latitude={:.2}&longitude={:.2}&current=weather_code,precipitation,snowfall,wind_speed_10m,wind_direction_10m,cloud_cover,is_day,temperature_2m&daily=sunrise,sunset&timezone=GMT&forecast_days=1&timeformat=unixtime",
        lat, lon
    )
}

/// The WMO code's class (`14` §3.3); unknown codes — `None`.
pub fn class_of(code: u32) -> Option<WeatherClass> {
    Some(match code {
        0 | 1 => WeatherClass::Clear,
        2 | 3 => WeatherClass::Cloudy,
        45 | 48 => WeatherClass::Fog,
        51..=67 | 80..=82 => WeatherClass::DrizzleRain,
        71..=77 | 85 | 86 => WeatherClass::Snow,
        95..=99 => WeatherClass::Storm,
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherState {
    pub fetched_at_ms: i64,
    pub lat: f64,
    pub lon: f64,
    pub code: u32,
    /// 0…1: rain and snow by their mm/h (5 mm/h — full), clouds and fog by the cloud cover.
    pub intensity: f32,
    /// m/s (Open-Meteo gives km/h).
    pub wind_ms: f32,
    /// Degrees the wind blows **from** (meteorological).
    pub wind_from_deg: f32,
    pub is_day: bool,
    pub temp_c: f32,
    /// Unix seconds of today's sunrise and sunset (UTC).
    pub sunrise_s: Option<i64>,
    pub sunset_s: Option<i64>,
}

/// Data older than this is stale (FR-CTX-002, FR-FX-002).
pub const STALE_MS: i64 = 3 * 3600 * 1000;

impl WeatherState {
    pub fn class(&self) -> Option<WeatherClass> {
        class_of(self.code)
    }

    pub fn fresh(&self, now_ms: i64) -> bool {
        (0..STALE_MS).contains(&(now_ms - self.fetched_at_ms))
    }

    /// The horizontal drift of particles, −1…1 (+ — to the right): the wind blows towards
    /// `from + 180°`; east on screen is right. Strong wind (≥ 12 m/s) — full.
    pub fn drift(&self) -> f32 {
        let to = (self.wind_from_deg + 180.0).to_radians();
        (to.sin() * (self.wind_ms / 12.0).min(1.0)).clamp(-1.0, 1.0)
    }
}

#[derive(Deserialize)]
struct Raw {
    current: Current,
    #[serde(default)]
    daily: Option<Daily>,
}

#[derive(Deserialize)]
struct Current {
    weather_code: u32,
    #[serde(default)]
    precipitation: f32,
    #[serde(default)]
    snowfall: f32,
    #[serde(default)]
    wind_speed_10m: f32,
    #[serde(default)]
    wind_direction_10m: f32,
    #[serde(default)]
    cloud_cover: f32,
    #[serde(default)]
    is_day: u8,
    #[serde(default)]
    temperature_2m: f32,
}

#[derive(Deserialize)]
struct Daily {
    #[serde(default)]
    sunrise: Vec<i64>,
    #[serde(default)]
    sunset: Vec<i64>,
}

/// Parses an Open-Meteo forecast answer for the point.
pub fn parse(json: &[u8], lat: f64, lon: f64, now_ms: i64) -> Option<WeatherState> {
    let r: Raw = serde_json::from_slice(json).ok()?;
    let c = r.current;
    let class = class_of(c.weather_code);
    let intensity = match class {
        Some(WeatherClass::Snow) => (c.snowfall * 0.7 / 5.0 + 0.3).min(1.0),
        Some(WeatherClass::DrizzleRain | WeatherClass::Storm) => {
            (c.precipitation / 5.0 + 0.3).min(1.0)
        }
        Some(WeatherClass::Cloudy | WeatherClass::Fog) => (c.cloud_cover / 100.0).clamp(0.3, 1.0),
        _ => 0.0,
    };
    Some(WeatherState {
        fetched_at_ms: now_ms,
        lat,
        lon,
        code: c.weather_code,
        intensity,
        wind_ms: c.wind_speed_10m / 3.6,
        wind_from_deg: c.wind_direction_10m,
        is_day: c.is_day != 0,
        temp_c: c.temperature_2m,
        sunrise_s: r.daily.as_ref().and_then(|d| d.sunrise.first().copied()),
        sunset_s: r.daily.as_ref().and_then(|d| d.sunset.first().copied()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SNOW: &str = r#"{"current":{"time":1793000000,"weather_code":73,"precipitation":0.4,
        "snowfall":1.4,"wind_speed_10m":18.0,"wind_direction_10m":270,"cloud_cover":100,
        "is_day":1,"temperature_2m":-3.2},"daily":{"sunrise":[1792990000],"sunset":[1793020000]}}"#;

    #[test]
    fn wmo_codes_map_to_the_six_classes() {
        assert_eq!(class_of(0), Some(WeatherClass::Clear));
        assert_eq!(class_of(3), Some(WeatherClass::Cloudy));
        assert_eq!(class_of(48), Some(WeatherClass::Fog));
        assert_eq!(class_of(61), Some(WeatherClass::DrizzleRain));
        assert_eq!(class_of(81), Some(WeatherClass::DrizzleRain));
        assert_eq!(class_of(75), Some(WeatherClass::Snow));
        assert_eq!(class_of(86), Some(WeatherClass::Snow));
        assert_eq!(class_of(95), Some(WeatherClass::Storm));
        assert_eq!(class_of(42), None);
    }

    #[test]
    fn a_snowy_answer_is_parsed_with_wind_and_sun() {
        let w = parse(SNOW.as_bytes(), 55.76, 37.62, 1_000).expect("parsed");
        assert_eq!(w.class(), Some(WeatherClass::Snow));
        assert!(w.intensity > 0.4 && w.intensity <= 1.0);
        assert!((w.wind_ms - 5.0).abs() < 0.01);
        // From the west → blows east → drifts right.
        assert!(w.drift() > 0.3, "{}", w.drift());
        assert!(w.is_day && w.temp_c < 0.0);
        assert_eq!(
            (w.sunrise_s, w.sunset_s),
            (Some(1_792_990_000), Some(1_793_020_000))
        );
        assert!(w.fresh(1_000 + STALE_MS - 1));
        assert!(!w.fresh(1_000 + STALE_MS));
    }

    #[test]
    fn the_query_rounds_coordinates() {
        let q = forecast_query(55.755_831, 37.617_673);
        assert!(q.contains("latitude=55.76&longitude=37.62"), "{q}");
    }

    #[test]
    fn garbage_is_none() {
        assert!(parse(b"{}", 0.0, 0.0, 0).is_none());
        assert!(parse(b"not json", 0.0, 0.0, 0).is_none());
    }
}
