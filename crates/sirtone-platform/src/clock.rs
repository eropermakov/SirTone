//! The real clock (docs/14 §3.1): UTC from the system, the local offset from the Windows time
//! zone rules for that instant (DST of that date, dynamic rules of that year).

use std::time::{SystemTime, UNIX_EPOCH};

use sirtone_core::wave::Clock;
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{
    DYNAMIC_TIME_ZONE_INFORMATION, FileTimeToSystemTime, GetDynamicTimeZoneInformation,
    SystemTimeToFileTime, SystemTimeToTzSpecificLocalTimeEx,
};

pub struct SystemClock;

/// 100-ns ticks between 1601-01-01 and 1970-01-01.
const EPOCH_DIFF: i64 = 116_444_736_000_000_000;

fn to_filetime(utc_ms: i64) -> FILETIME {
    let t = (utc_ms * 10_000 + EPOCH_DIFF) as u64;
    FILETIME {
        dwLowDateTime: t as u32,
        dwHighDateTime: (t >> 32) as u32,
    }
}

fn from_filetime(f: FILETIME) -> i64 {
    let t = (u64::from(f.dwHighDateTime) << 32 | u64::from(f.dwLowDateTime)) as i64;
    (t - EPOCH_DIFF) / 10_000
}

/// Local offset at `utc_ms` in minutes, or `None` when Windows cannot say.
pub fn offset_at(utc_ms: i64) -> Option<i32> {
    unsafe {
        let mut tz = DYNAMIC_TIME_ZONE_INFORMATION::default();
        GetDynamicTimeZoneInformation(&mut tz);
        let mut utc = SYSTEMTIME::default();
        FileTimeToSystemTime(&to_filetime(utc_ms), &mut utc).ok()?;
        let mut local = SYSTEMTIME::default();
        SystemTimeToTzSpecificLocalTimeEx(Some(&tz), &utc, &mut local).ok()?;
        let mut ft = FILETIME::default();
        SystemTimeToFileTime(&local, &mut ft).ok()?;
        Some(((from_filetime(ft) - utc_ms) as f64 / 60_000.0).round() as i32)
    }
}

impl Clock for SystemClock {
    fn now_utc_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64)
    }
    fn local_offset_min(&self, utc_ms: i64) -> i32 {
        offset_at(utc_ms).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_roundtrip() {
        for ms in [0i64, 1_792_000_000_000, 1_000_123] {
            assert_eq!(from_filetime(to_filetime(ms)), ms);
        }
    }

    #[test]
    fn offset_is_plausible_and_whole_quarter_hours() {
        let now = SystemClock.now_utc_ms();
        let off = SystemClock.local_offset_min(now);
        assert!((-12 * 60..=14 * 60).contains(&off));
        assert_eq!(off % 15, 0);
    }
}
