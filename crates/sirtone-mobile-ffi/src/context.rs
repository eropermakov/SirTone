//! «Обстановка» on the phone (D-149, D-150, `mobile/full-port.md` §2): what the phone's signals
//! mean for «Моя волна». The app reads the sensors (Core Motion, the audio route, places it asked
//! about, Open-Meteo); this decides from `assets/wave/phone_context.json` — the same activity
//! keys as the PC's «Занятие по активному приложению», never over a chosen one.

use std::sync::OnceLock;

/// The thresholds the app needs (from the JSON).
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct ContextRules {
    /// A motion must hold this long before the wave follows it (no flip at a traffic light).
    pub stable_s: u32,
    /// «Долго еду» after this many minutes in a car.
    pub long_drive_min: u32,
    /// A place («Дом», «Работа») covers this radius.
    pub place_radius_m: u32,
    /// Ask «Это дом?» / «Это работа?» after this long in one unnamed spot at those hours.
    pub ask_after_min: u32,
    pub night_from_h: u8,
    pub night_to_h: u8,
    pub work_from_h: u8,
    pub work_to_h: u8,
    /// Weather is asked again after this long.
    pub weather_every_min: u32,
}

fn doc() -> &'static serde_json::Value {
    static D: OnceLock<serde_json::Value> = OnceLock::new();
    D.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/wave/phone_context.json"))
            .unwrap_or_default()
    })
}

fn num(k: &str, default: u64) -> u64 {
    doc()
        .get(k)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(default)
}

fn hour(k: &str, default: u8) -> u8 {
    u8::try_from(num(k, u64::from(default))).unwrap_or(default)
}

#[uniffi::export]
pub fn context_rules() -> ContextRules {
    let n = |k, d| u32::try_from(num(k, d)).unwrap_or(u32::MAX);
    ContextRules {
        stable_s: n("stable_s", 120),
        long_drive_min: n("long_drive_min", 20),
        place_radius_m: n("place_radius_m", 200),
        ask_after_min: n("ask_after_min", 30),
        night_from_h: hour("night_from_h", 22),
        night_to_h: hour("night_to_h", 6),
        work_from_h: hour("work_from_h", 9),
        work_to_h: hour("work_to_h", 18),
        weather_every_min: n("weather_every_min", 45),
    }
}

/// The activity for «Моя волна» now. `motion` — `stationary` / `walking` / `running` /
/// `cycling` / `automotive` / `unknown` (already stable for `stable_s`); `car` — the sound goes
/// to CarPlay or a Bluetooth device marked «Это машина»; `place` — `home` / `work` / none.
/// Moving wins over the place; the car's audio confirms «в машине» (Core Motion alone is
/// unreliable there, `01d` M55).
#[uniffi::export]
pub fn auto_activity(
    motion: String,
    car: bool,
    place: Option<String>,
    local_hour: u8,
) -> Option<String> {
    let d = doc();
    let key =
        |v: Option<&serde_json::Value>| v.and_then(serde_json::Value::as_str).map(str::to_owned);
    if car {
        return key(d.get("car_audio"));
    }
    if motion != "automotive"
        && let Some(a) = key(d.get("motion").and_then(|m| m.get(&motion)))
    {
        return Some(a);
    }
    match place.as_deref() {
        Some("work") => key(d.pointer("/place/work")),
        Some("home") if local_hour >= hour("home_evening_from_h", 18) => {
            key(d.pointer("/place/home_evening"))
        }
        _ => None,
    }
}

/// The Open-Meteo request for a place, coordinates rounded to `coord_step_deg` (0.1° ≈ 10 km,
/// D-149: nothing finer leaves the phone). The app prefixes the host (`08` §4).
#[uniffi::export]
pub fn weather_query(lat: f64, lon: f64) -> String {
    let (lat, lon) = coarse(lat, lon);
    sirtone_wave::weather::forecast_query(lat, lon)
}

pub(crate) fn coarse(lat: f64, lon: f64) -> (f64, f64) {
    let step = doc()
        .get("coord_step_deg")
        .and_then(serde_json::Value::as_f64)
        .filter(|s| *s > 0.0)
        .unwrap_or(0.1);
    ((lat / step).round() * step, (lon / step).round() * step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_car_and_places_map_to_activities() {
        assert_eq!(
            auto_activity("walking".into(), false, None, 12).as_deref(),
            Some("road")
        );
        assert_eq!(
            auto_activity("running".into(), false, None, 12).as_deref(),
            Some("workout")
        );
        // Core Motion's «automotive» alone does not decide; the car's audio does.
        assert_eq!(auto_activity("automotive".into(), false, None, 12), None);
        assert_eq!(
            auto_activity("stationary".into(), true, None, 12).as_deref(),
            Some("road")
        );
        assert_eq!(
            auto_activity("stationary".into(), false, Some("work".into()), 11).as_deref(),
            Some("work")
        );
        assert_eq!(
            auto_activity("stationary".into(), false, Some("home".into()), 10),
            None
        );
        assert_eq!(
            auto_activity("stationary".into(), false, Some("home".into()), 20).as_deref(),
            Some("rest")
        );
        let r = context_rules();
        assert_eq!((r.stable_s, r.long_drive_min), (120, 20));
    }

    /// D-149: only ~10 km precision leaves the phone.
    #[test]
    fn weather_asks_with_coarse_coordinates() {
        let q = weather_query(55.75583, 37.61730);
        assert!(q.contains("latitude=55.80&longitude=37.60"), "{q}");
    }
}
