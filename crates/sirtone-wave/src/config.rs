//! Wave configs from `assets/wave/*.json` (docs/14 §22): numbers live in data, not in code. The
//! shipped files are compiled in as defaults; `from_json` accepts edited copies.

use serde::Deserialize;
use sirtone_core::wave::Daypart;

#[derive(Debug, Clone, Deserialize)]
pub struct SeasonsConfig {
    pub schema: String,
    /// Transition window ±days around a season boundary (Q-10).
    pub blend_days: i32,
    /// Weather may move season weights by at most this much.
    pub weather_cap: f32,
    /// Which weather moves which season (FR-SEAS-001).
    #[serde(default)]
    pub weather_nudge: Vec<NudgeRule>,
}

/// One weather → season rule: a weather class, or a temperature at/above `min_c` or at/below
/// `max_c`, adds `amount` to `season` (`winter` · `spring` · `summer` · `autumn`).
#[derive(Debug, Clone, Deserialize)]
pub struct NudgeRule {
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub min_c: Option<f32>,
    #[serde(default)]
    pub max_c: Option<f32>,
    pub season: String,
    pub amount: f32,
}

#[derive(Debug, Clone, Deserialize)]
struct DaypartFrom {
    daypart: String,
    from: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ContextJson {
    schema: String,
    daypart_by_clock: Vec<DaypartFrom>,
    daypart_by_sun: BySunJson,
    #[serde(default)]
    activity_apps: std::collections::BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct BySunJson {
    dawn_before_rise: u16,
    morning_after_rise: u16,
    day_from: String,
    evening_before_set: u16,
    dusk_before_set: u16,
    night_after_set: u16,
}

/// Daypart boundaries around the sun (minutes), `context.json` `daypart_by_sun`.
#[derive(Debug, Clone, Copy)]
pub struct BySun {
    pub dawn_before_rise: u16,
    pub morning_after_rise: u16,
    pub day_from: u16,
    pub evening_before_set: u16,
    pub dusk_before_set: u16,
    pub night_after_set: u16,
}

/// Daypart boundaries by the clock: `(start minute, daypart)`, ascending.
#[derive(Debug, Clone)]
pub struct ContextConfig {
    pub daypart_by_clock: Vec<(u16, Daypart)>,
    pub by_sun: BySun,
    /// «Занятие» by the foreground application (FR-CTX-003): activity key → exe names.
    pub activity_apps: Vec<(String, Vec<String>)>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unexpected schema {0}")]
    Schema(String),
    #[error("invalid value: {0}")]
    Invalid(String),
}

fn daypart(name: &str) -> Option<Daypart> {
    Some(match name {
        "dawn" => Daypart::Dawn,
        "morning" => Daypart::Morning,
        "day" => Daypart::Day,
        "evening" => Daypart::Evening,
        "dusk" => Daypart::Dusk,
        "night" => Daypart::Night,
        _ => return None,
    })
}

fn hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl SeasonsConfig {
    pub fn from_json(s: &str) -> Result<Self, ConfigError> {
        let c: Self = serde_json::from_str(s)?;
        if c.schema != "sirtone.seasons/1" {
            return Err(ConfigError::Schema(c.schema));
        }
        if !(0..=45).contains(&c.blend_days) || !(0.0..=1.0).contains(&c.weather_cap) {
            return Err(ConfigError::Invalid("blend_days/weather_cap".into()));
        }
        Ok(c)
    }
}

impl Default for SeasonsConfig {
    // The shipped file is compiled in and checked by `shipped_configs_parse`.
    #[allow(clippy::expect_used)]
    fn default() -> Self {
        Self::from_json(include_str!("../../../assets/wave/seasons.json"))
            .expect("shipped seasons.json is valid (test)")
    }
}

impl ContextConfig {
    /// The activity an application stands for (`code.exe` → `work`); `None` — says nothing.
    pub fn activity_of(&self, exe: &str) -> Option<&str> {
        let exe = exe.to_lowercase();
        self.activity_apps
            .iter()
            .find(|(_, apps)| apps.contains(&exe))
            .map(|(k, _)| k.as_str())
    }

    pub fn from_json(s: &str) -> Result<Self, ConfigError> {
        let c: ContextJson = serde_json::from_str(s)?;
        if c.schema != "sirtone.context/1" {
            return Err(ConfigError::Schema(c.schema));
        }
        let mut v = Vec::with_capacity(c.daypart_by_clock.len());
        for e in &c.daypart_by_clock {
            let (Some(at), Some(dp)) = (hhmm(&e.from), daypart(&e.daypart)) else {
                return Err(ConfigError::Invalid(format!("{} {}", e.daypart, e.from)));
            };
            v.push((at, dp));
        }
        if v.is_empty() || v.windows(2).any(|w| w[0].0 >= w[1].0) {
            return Err(ConfigError::Invalid("daypart_by_clock must ascend".into()));
        }
        let s = &c.daypart_by_sun;
        let Some(day_from) = hhmm(&s.day_from) else {
            return Err(ConfigError::Invalid(format!("day_from {}", s.day_from)));
        };
        Ok(Self {
            activity_apps: c
                .activity_apps
                .iter()
                .map(|(k, v)| (k.clone(), v.iter().map(|e| e.to_lowercase()).collect()))
                .collect(),
            daypart_by_clock: v,
            by_sun: BySun {
                dawn_before_rise: s.dawn_before_rise,
                morning_after_rise: s.morning_after_rise,
                day_from,
                evening_before_set: s.evening_before_set,
                dusk_before_set: s.dusk_before_set,
                night_after_set: s.night_after_set,
            },
        })
    }

    /// The daypart by the sun (FR-CTX-001): `rise`/`set` — local minutes of today's sunrise and
    /// sunset. Boundaries keep their order even on short winter days (each clamped after the last).
    pub fn daypart_by_sun(&self, minute: u16, rise: f64, set: f64) -> Daypart {
        let b = &self.by_sun;
        let m = f64::from(minute);
        let dawn = rise - f64::from(b.dawn_before_rise);
        let morning = (rise + f64::from(b.morning_after_rise)).max(dawn);
        let evening = (set - f64::from(b.evening_before_set)).max(morning);
        let day = f64::from(b.day_from).clamp(morning, evening);
        let dusk = (set - f64::from(b.dusk_before_set)).max(evening);
        let night = (set + f64::from(b.night_after_set)).max(dusk);
        if m < dawn || m >= night {
            Daypart::Night
        } else if m < morning {
            Daypart::Dawn
        } else if m < day {
            Daypart::Morning
        } else if m < evening {
            Daypart::Day
        } else if m < dusk {
            Daypart::Evening
        } else {
            Daypart::Dusk
        }
    }

    /// The daypart at `minute` past local midnight; before the first boundary the last one
    /// (night) continues from the previous evening.
    pub fn daypart_at(&self, minute: u16) -> Daypart {
        self.daypart_by_clock
            .iter()
            .rev()
            .find(|(at, _)| *at <= minute)
            .or(self.daypart_by_clock.last())
            .map_or(Daypart::Night, |(_, d)| *d)
    }
}

impl Default for ContextConfig {
    #[allow(clippy::expect_used)]
    fn default() -> Self {
        Self::from_json(include_str!("../../../assets/wave/context.json"))
            .expect("shipped context.json is valid (test)")
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn an_application_stands_for_an_activity() {
        let c = ContextConfig::default();
        assert_eq!(c.activity_of("Code.exe"), Some("work"));
        assert_eq!(c.activity_of("chrome.exe"), None);
    }

    use super::*;

    #[test]
    fn shipped_configs_parse() {
        let s = SeasonsConfig::default();
        assert_eq!((s.blend_days, s.weather_cap), (21, 0.3));
        let c = ContextConfig::default();
        assert_eq!(c.daypart_at(0), Daypart::Night);
        assert_eq!(c.daypart_at(5 * 60), Daypart::Dawn);
        assert_eq!(c.daypart_at(9 * 60), Daypart::Morning);
        assert_eq!(c.daypart_at(13 * 60), Daypart::Day);
        assert_eq!(c.daypart_at(18 * 60), Daypart::Evening);
        assert_eq!(c.daypart_at(21 * 60 + 30), Daypart::Dusk);
        assert_eq!(c.daypart_at(23 * 60 + 59), Daypart::Night);
    }

    #[test]
    fn bad_configs_are_rejected() {
        assert!(
            SeasonsConfig::from_json(r#"{"schema":"x","blend_days":21,"weather_cap":0.3}"#)
                .is_err()
        );
        assert!(
            ContextConfig::from_json(
                r#"{"schema":"sirtone.context/1","daypart_by_clock":[{"daypart":"day","from":"12:00"},{"daypart":"dawn","from":"05:00"}]}"#
            )
            .is_err()
        );
        assert!(
            ContextConfig::from_json(
                r#"{"schema":"sirtone.context/1","daypart_by_clock":[{"daypart":"noon","from":"12:00"}]}"#
            )
            .is_err()
        );
    }
}
