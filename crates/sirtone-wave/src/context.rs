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
}

impl ContextBuilder {
    pub fn snapshot(&self, clock: &dyn Clock) -> ContextSnapshot {
        self.snapshot_at(clock, clock.now_utc_ms())
    }

    pub fn snapshot_at(&self, clock: &dyn Clock, utc_ms: i64) -> ContextSnapshot {
        let t = local_time(clock, utc_ms);
        ContextSnapshot {
            now_utc_ms: utc_ms,
            local_minute: t.minute,
            weekday: t.weekday,
            daypart: self.context.daypart_at(t.minute),
            season_w: season_weights(t.day, self.southern, self.seasons.blend_days),
            weather: None,
        }
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
}
