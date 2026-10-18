//! Substitutable clocks (docs/14 §3.1). The system clock lives in `sirtone-platform` (time zones
//! need Windows); here are the ones for tests and `wave simulate`.

use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

use sirtone_core::wave::Clock;

use crate::calendar::{civil_from_days, weekday};

/// A frozen moment with a fixed offset (tests, AT-24 «date substitution»).
pub struct FixedClock {
    utc_ms: AtomicI64,
    offset_min: i32,
}

impl FixedClock {
    pub fn new(utc_ms: i64, offset_min: i32) -> Self {
        Self {
            utc_ms: AtomicI64::new(utc_ms),
            offset_min,
        }
    }

    /// Local date and time → clock (`offset_min` east of UTC).
    pub fn local(y: i32, m: u32, d: u32, h: u32, min: u32, offset_min: i32) -> Self {
        let days = i64::from(crate::calendar::days_from_civil(y, m, d));
        let local_ms = days * 86_400_000 + i64::from(h * 60 + min) * 60_000;
        Self::new(local_ms - i64::from(offset_min) * 60_000, offset_min)
    }

    pub fn set(&self, utc_ms: i64) {
        self.utc_ms.store(utc_ms, Ordering::Relaxed);
    }

    pub fn advance_ms(&self, ms: i64) {
        self.utc_ms.fetch_add(ms, Ordering::Relaxed);
    }
}

impl Clock for FixedClock {
    fn now_utc_ms(&self) -> i64 {
        self.utc_ms.load(Ordering::Relaxed)
    }
    fn local_offset_min(&self, _utc_ms: i64) -> i32 {
        self.offset_min
    }
}

/// Time running `speed` times faster from `start_utc_ms` (debug setting, AT-24).
pub struct AcceleratedClock<C: Clock> {
    inner: C,
    start_utc_ms: i64,
    speed: f64,
    started: Instant,
}

impl<C: Clock> AcceleratedClock<C> {
    pub fn new(inner: C, start_utc_ms: i64, speed: f64) -> Self {
        Self {
            inner,
            start_utc_ms,
            speed,
            started: Instant::now(),
        }
    }
}

impl<C: Clock> Clock for AcceleratedClock<C> {
    fn now_utc_ms(&self) -> i64 {
        self.start_utc_ms + (self.started.elapsed().as_millis() as f64 * self.speed) as i64
    }
    fn local_offset_min(&self, utc_ms: i64) -> i32 {
        self.inner.local_offset_min(utc_ms)
    }
}

/// A moment in local terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    /// Local calendar day, days since 1970-01-01.
    pub day: i32,
    pub year: i32,
    pub month: u32,
    pub dom: u32,
    /// Minutes since local midnight.
    pub minute: u16,
    /// 0 = Monday.
    pub weekday: u8,
    pub offset_min: i32,
}

pub fn local_time(clock: &dyn Clock, utc_ms: i64) -> LocalTime {
    let offset_min = clock.local_offset_min(utc_ms);
    let local_ms = utc_ms + i64::from(offset_min) * 60_000;
    let day = local_ms.div_euclid(86_400_000) as i32;
    let minute = (local_ms.rem_euclid(86_400_000) / 60_000) as u16;
    let (year, month, dom) = civil_from_days(day);
    LocalTime {
        day,
        year,
        month,
        dom,
        minute,
        weekday: weekday(day) as u8,
        offset_min,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_time_crosses_midnight_by_offset() {
        // 31.12.2026 23:30 in Moscow = 20:30 UTC
        let c = FixedClock::local(2026, 12, 31, 23, 30, 180);
        let t = local_time(&c, c.now_utc_ms());
        assert_eq!(
            (t.year, t.month, t.dom, t.minute),
            (2026, 12, 31, 23 * 60 + 30)
        );
        assert_eq!(t.weekday, 3); // Thursday
        c.advance_ms(60 * 60_000);
        let t = local_time(&c, c.now_utc_ms());
        assert_eq!((t.year, t.month, t.dom, t.minute), (2027, 1, 1, 30));
    }

    #[test]
    fn accelerated_clock_runs_fast() {
        let a = AcceleratedClock::new(FixedClock::new(0, 0), 1_000, 3_600.0);
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(a.now_utc_ms() >= 1_000 + 60_000);
    }
}
