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
}

/// Daypart boundaries by the clock: `(start minute, daypart)`, ascending.
#[derive(Debug, Clone)]
pub struct ContextConfig {
    pub daypart_by_clock: Vec<(u16, Daypart)>,
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
        Ok(Self {
            daypart_by_clock: v,
        })
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
