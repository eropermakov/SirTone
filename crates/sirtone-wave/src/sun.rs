//! Sunrise and sunset (T5.5, FR-CTX-001, `05` §10.2): the NOAA solar calculator's equations
//! (Jean Meeus, «Astronomical Algorithms»; the same method as the `sunrise` crate), computed
//! locally, no network. Times are minutes from UTC midnight of the given day; the caller adds the
//! local offset from `Clock`. Polar day or night — `None` (the clock dayparts then apply).

use std::f64::consts::PI;

/// The Sun's centre 0.833° below the horizon: refraction + the solar disc (the usual definition).
const ZENITH_DEG: f64 = 90.833;

fn rad(d: f64) -> f64 {
    d * PI / 180.0
}

fn deg(r: f64) -> f64 {
    r * 180.0 / PI
}

/// (sunrise, sunset) in minutes after 00:00 UTC of `days` (days since 1970-01-01).
pub fn sun_times(lat: f64, lon: f64, days: i32) -> Option<(f64, f64)> {
    // Julian day at noon UTC, then the Julian century.
    let jd = f64::from(days) + 2_440_587.5 + 0.5;
    let t = (jd - 2_451_545.0) / 36_525.0;
    let l0 = (280.466_46 + t * (36_000.769_83 + t * 0.000_303_2)).rem_euclid(360.0);
    let m = 357.529_11 + t * (35_999.050_29 - 0.000_153_7 * t);
    let e = 0.016_708_634 - t * (0.000_042_037 + 0.000_000_126_7 * t);
    let c = rad(m).sin() * (1.914_602 - t * (0.004_817 + 0.000_014 * t))
        + rad(2.0 * m).sin() * (0.019_993 - 0.000_101 * t)
        + rad(3.0 * m).sin() * 0.000_289;
    let true_long = l0 + c;
    let omega = 125.04 - 1934.136 * t;
    let app_long = true_long - 0.005_69 - 0.004_78 * rad(omega).sin();
    let eps0 =
        23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.000_59 - t * 0.001_813))) / 60.0) / 60.0;
    let eps = eps0 + 0.002_56 * rad(omega).cos();
    let decl = deg((rad(eps).sin() * rad(app_long).sin()).asin());
    let y = (rad(eps / 2.0)).tan().powi(2);
    let eq_time = 4.0
        * deg(y * (2.0 * rad(l0)).sin() - 2.0 * e * rad(m).sin()
            + 4.0 * e * y * rad(m).sin() * (2.0 * rad(l0)).cos()
            - 0.5 * y * y * (4.0 * rad(l0)).sin()
            - 1.25 * e * e * (2.0 * rad(m)).sin());
    let cos_ha = rad(ZENITH_DEG).cos() / (rad(lat).cos() * rad(decl).cos())
        - rad(lat).tan() * rad(decl).tan();
    if !(-1.0..=1.0).contains(&cos_ha) {
        return None;
    }
    let ha = deg(cos_ha.acos());
    let noon = 720.0 - 4.0 * lon - eq_time;
    Some((noon - 4.0 * ha, noon + 4.0 * ha))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::days_from_civil;

    fn hm(min: f64) -> (i32, i32) {
        let m = min.rem_euclid(1440.0).round() as i32;
        (m / 60, m % 60)
    }

    #[test]
    fn moscow_and_sydney_match_the_almanac_within_two_minutes() {
        // Moscow 55.7558, 37.6173; 2026-06-21: sunrise 03:44, sunset 21:18 MSK (UTC+3).
        let (r, s) = sun_times(55.7558, 37.6173, days_from_civil(2026, 6, 21)).unwrap_or_default();
        let near = |got: f64, h: i32, m: i32| {
            let (gh, gm) = hm(got + 180.0);
            ((gh * 60 + gm) - (h * 60 + m)).abs() <= 2
        };
        assert!(near(r, 3, 44), "rise {:?}", hm(r + 180.0));
        assert!(near(s, 21, 18), "set {:?}", hm(s + 180.0));
        // Winter: 2026-12-21 — 08:59 / 15:57 MSK.
        let (r, s) = sun_times(55.7558, 37.6173, days_from_civil(2026, 12, 21)).unwrap_or_default();
        assert!(near(r, 8, 59), "rise {:?}", hm(r + 180.0));
        assert!(near(s, 15, 57), "set {:?}", hm(s + 180.0));
        // Sydney −33.8688, 151.2093; 2026-12-21: 05:41 / 20:05 AEDT (UTC+11).
        let (r, s) =
            sun_times(-33.8688, 151.2093, days_from_civil(2026, 12, 21)).unwrap_or_default();
        let (rh, rm) = hm(r + 660.0);
        let (sh, sm) = hm(s + 660.0);
        assert!(((rh * 60 + rm) - (5 * 60 + 41)).abs() <= 2, "{rh}:{rm}");
        assert!(((sh * 60 + sm) - (20 * 60 + 5)).abs() <= 2, "{sh}:{sm}");
    }

    #[test]
    fn polar_day_and_night_have_no_sunrise() {
        // Murmansk 68.97: midnight sun in June, polar night in December.
        assert!(sun_times(68.97, 33.07, days_from_civil(2026, 6, 21)).is_none());
        assert!(sun_times(68.97, 33.07, days_from_civil(2026, 12, 21)).is_none());
    }
}
