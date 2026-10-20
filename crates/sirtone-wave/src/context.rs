//! `ContextSnapshot` v1 (docs/14 §3.2): clock-only fields — time, daypart by the clock, season
//! blend. Sun position, weather and events join in Stage 5.

use sirtone_core::wave::{Clock, ContextSnapshot};

use crate::calendar::season_weights;
use crate::clock::local_time;
use crate::config::{ContextConfig, SeasonsConfig};

#[derive(Debug, Clone, Default)]
pub struct ContextBuilder {
    pub seasons: SeasonsConfig,
    pub context: ContextConfig,
    /// Southern hemisphere (from the chosen city's latitude; none chosen → northern).
    pub southern: bool,
    /// The chosen city (T5.5): the daypart by the sun; `None` — by the clock.
    pub coords: Option<(f64, f64)>,
    /// The last weather; only a fresh one (< 3 h) enters the snapshot (AT-16).
    pub weather: Option<crate::weather::WeatherState>,
}

impl ContextBuilder {
    pub fn snapshot(&self, clock: &dyn Clock) -> ContextSnapshot {
        self.snapshot_at(clock, clock.now_utc_ms())
    }

    pub fn snapshot_at(&self, clock: &dyn Clock, utc_ms: i64) -> ContextSnapshot {
        let t = local_time(clock, utc_ms);
        let offset = f64::from(clock.local_offset_min(utc_ms));
        let by_sun = self.coords.and_then(|(lat, lon)| {
            let (rise, set) = crate::sun::sun_times(lat, lon, t.day)?;
            Some(
                self.context
                    .daypart_by_sun(t.minute, rise + offset, set + offset),
            )
        });
        let weather = self
            .weather
            .as_ref()
            .filter(|w| w.fresh(utc_ms))
            .and_then(crate::weather::WeatherState::class);
        ContextSnapshot {
            now_utc_ms: utc_ms,
            local_minute: t.minute,
            weekday: t.weekday,
            daypart: by_sun.unwrap_or_else(|| self.context.daypart_at(t.minute)),
            season_w: season_weights(t.day, self.southern, self.seasons.blend_days),
            weather,
        }
    }
}

/// The context bucket of «не сейчас» and the context model (`14` §8.2 levels: daypart →
/// × season → × weather; D-77): `d{daypart}/s{season}/w{weather|x}`, the season being the
/// dominant one of the weights, `x` — weather unknown.
pub fn bucket_key(ctx: &ContextSnapshot) -> String {
    let season = ctx
        .season_w
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(i, _)| i);
    let weather = ctx
        .weather
        .map_or_else(|| "x".to_string(), |w| (w as u8).to_string());
    format!("d{}/s{season}/w{weather}", ctx.daypart as u8)
}

/// Whether a stored bucket applies now: same daypart and season; weather equal or unknown on
/// either side. Legacy `daypart:N` rows (prototypes 0.21–0.23) are daypart-only keys.
pub fn bucket_matches(stored: &str, now: &ContextSnapshot) -> bool {
    if let Some(d) = stored.strip_prefix("daypart:") {
        return d.parse::<u8>().is_ok_and(|d| d == now.daypart as u8);
    }
    let cur = bucket_key(now);
    let parts = |k: &str| -> Option<(String, String, String)> {
        let mut it = k.split('/');
        Some((
            it.next()?.to_string(),
            it.next()?.to_string(),
            it.next()?.to_string(),
        ))
    };
    match (parts(stored), parts(&cur)) {
        (Some((d1, s1, w1)), Some((d2, s2, w2))) => {
            d1 == d2 && s1 == s2 && (w1 == w2 || w1 == "wx" || w2 == "wx")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FixedClock;
    use sirtone_core::wave::Daypart;

    #[test]
    fn predzimye_evening_in_moscow() {
        // AT-14: 22 November, 19:00 Moscow — evening, winter already noticeable
        let c = FixedClock::local(2026, 11, 22, 19, 0, 180);
        let s = ContextBuilder::default().snapshot(&c);
        assert_eq!(s.daypart, Daypart::Evening);
        assert_eq!(s.weekday, 6); // Sunday
        assert_eq!(s.local_minute, 19 * 60);
        assert!(
            s.season_w[0] > 0.25 && s.season_w[3] > 0.6,
            "{:?}",
            s.season_w
        );
    }

    #[test]
    fn the_daypart_follows_the_sun_where_the_city_is_known() {
        use sirtone_core::wave::Daypart;
        let moscow = ContextBuilder {
            coords: Some((55.7558, 37.6173)),
            ..ContextBuilder::default()
        };
        // 21 December: sunset 15:57 — at 16:30 it is dusk by the sun, still «day» by the clock.
        let c = FixedClock::local(2026, 12, 21, 16, 30, 180);
        assert_eq!(moscow.snapshot(&c).daypart, Daypart::Dusk);
        assert_eq!(ContextBuilder::default().snapshot(&c).daypart, Daypart::Day);
        // 08:30 that day — dawn (sunrise 08:59).
        let c = FixedClock::local(2026, 12, 21, 8, 30, 180);
        assert_eq!(moscow.snapshot(&c).daypart, Daypart::Dawn);
        // 21 June, 21:00 — still evening (sunset 21:18), 23:00 — night.
        let c = FixedClock::local(2026, 6, 21, 21, 0, 180);
        assert_eq!(moscow.snapshot(&c).daypart, Daypart::Dusk);
        let c = FixedClock::local(2026, 6, 21, 23, 0, 180);
        assert_eq!(moscow.snapshot(&c).daypart, Daypart::Night);
    }

    #[test]
    fn only_fresh_weather_enters_the_snapshot() {
        use sirtone_core::wave::WeatherClass;
        let c = FixedClock::local(2026, 12, 21, 12, 0, 180);
        let now = c.now_utc_ms();
        let w = |age_ms: i64| crate::weather::WeatherState {
            fetched_at_ms: now - age_ms,
            lat: 55.76,
            lon: 37.62,
            code: 73,
            intensity: 0.5,
            wind_ms: 3.0,
            wind_from_deg: 270.0,
            is_day: true,
            temp_c: -3.0,
            sunrise_s: None,
            sunset_s: None,
        };
        let b = |age| ContextBuilder {
            weather: Some(w(age)),
            ..ContextBuilder::default()
        };
        assert_eq!(b(60_000).snapshot(&c).weather, Some(WeatherClass::Snow));
        assert_eq!(b(3 * 3600 * 1000 + 1).snapshot(&c).weather, None);
    }

    #[test]
    fn local_day_decides_the_season() {
        // 01:00 on 1 March in Moscow is still 28 February in UTC: the local date counts
        let c = FixedClock::local(2027, 3, 1, 1, 0, 180);
        let b = ContextBuilder {
            seasons: SeasonsConfig {
                blend_days: 0,
                ..SeasonsConfig::default()
            },
            ..ContextBuilder::default()
        };
        assert_eq!(b.snapshot(&c).season_w, [0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn bucket_keys_and_matching() {
        use sirtone_core::wave::{Daypart, WeatherClass};
        let mut ctx = ContextSnapshot {
            now_utc_ms: 0,
            local_minute: 20 * 60,
            weekday: 2,
            daypart: Daypart::Evening,
            season_w: [0.1, 0.0, 0.0, 0.9],
            weather: Some(WeatherClass::DrizzleRain),
        };
        let key = super::bucket_key(&ctx);
        assert_eq!(key, format!("d{}/s3/w3", Daypart::Evening as u8));
        assert!(super::bucket_matches(&key, &ctx));
        // Weather unknown now: the stored rainy evening still applies.
        ctx.weather = None;
        assert!(super::bucket_matches(&key, &ctx));
        // Another season does not.
        ctx.season_w = [0.9, 0.0, 0.0, 0.1];
        assert!(!super::bucket_matches(&key, &ctx));
        // Legacy keys from 0.21–0.23: daypart only.
        let legacy = format!("daypart:{}", Daypart::Evening as u8);
        assert!(super::bucket_matches(&legacy, &ctx));
        ctx.daypart = Daypart::Morning;
        assert!(!super::bucket_matches(&legacy, &ctx));
    }
}
