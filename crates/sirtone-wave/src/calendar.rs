//! Offline calendar of the wave engine (docs/14 §10–11, ported from spike S-17): dates as days
//! since 1970-01-01 (Howard Hinnant's algorithms), Easter (Meeus/Jones/Butcher and Meeus'
//! Julian algorithm), Maslenitsa, the n-th weekday of a month, season blending with a
//! transition window, equinoxes and solstices (Meeus ch. 27 mean formulas, minutes accuracy).

use sirtone_core::wave::Season;

/// Дни от 1970-01-01 для григорианской даты.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i32 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i32 - 719_468
}

/// Обратное преобразование: (год, месяц, день).
pub fn civil_from_days(z: i32) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// День недели: 0 = понедельник … 6 = воскресенье (1970-01-01 — четверг).
pub fn weekday(days: i32) -> u32 {
    (days + 3).rem_euclid(7) as u32
}

/// Западная (григорианская) Пасха — анонимный алгоритм Meeus/Jones/Butcher.
pub fn easter_western(y: i32) -> i32 {
    let a = y % 19;
    let b = y / 100;
    let c = y % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    days_from_civil(y, month as u32, day as u32)
}

/// Православная Пасха в григорианском календаре: юлианский алгоритм Меёса + разница календарей
/// (13 дней в 1900–2099; формула верна для весенних дат любого века).
pub fn easter_orthodox(y: i32) -> i32 {
    let a = y % 4;
    let b = y % 7;
    let c = y % 19;
    let d = (19 * c + 15) % 30;
    let e = (2 * a + 4 * b - d + 34) % 7;
    let month = (d + e + 114) / 31;
    let day = (d + e + 114) % 31 + 1;
    let julian_to_gregorian = y / 100 - y / 400 - 2;
    days_from_civil(y, month as u32, day as u32) + julian_to_gregorian
}

/// Масленичная неделя: понедельник … Прощёное воскресенье (= православная Пасха − 49 дней).
pub fn maslenitsa(y: i32) -> (i32, i32) {
    let sunday = easter_orthodox(y) - 49;
    (sunday - 6, sunday)
}

/// Days in a month of the Gregorian calendar.
pub fn days_in_month(y: i32, m: u32) -> u32 {
    let next = if m == 12 {
        days_from_civil(y + 1, 1, 1)
    } else {
        days_from_civil(y, m + 1, 1)
    };
    (next - days_from_civil(y, m, 1)) as u32
}

/// The `n`-th `weekday` (0 = Monday) of a month; `n = -1` is the last one. `None` when the
/// month has no such day (a 5th Monday that does not exist).
pub fn nth_weekday(y: i32, m: u32, wd: u32, n: i32) -> Option<i32> {
    let first = days_from_civil(y, m, 1);
    let last = first + days_in_month(y, m) as i32 - 1;
    let day = if n > 0 {
        let first_match = first + (wd as i32 - weekday(first) as i32).rem_euclid(7);
        first_match + 7 * (n - 1)
    } else if n == -1 {
        last - (weekday(last) as i32 - wd as i32).rem_euclid(7)
    } else {
        return None;
    };
    (first..=last).contains(&day).then_some(day)
}

/// Equinox / solstice: 0 — March equinox, 1 — June solstice, 2 — September equinox,
/// 3 — December solstice.
pub fn astro_utc_ms(year: i32, which: u8) -> i64 {
    ((equinox_solstice_jde(year, which) - 2_440_587.5) * 86_400_000.0).round() as i64
}

/// The local calendar day of an astronomical moment (a solstice at 23:50 UTC is the next day in
/// Moscow).
pub fn astro_local_day(year: i32, which: u8, offset_min: i32) -> i32 {
    (astro_utc_ms(year, which) + i64::from(offset_min) * 60_000).div_euclid(86_400_000) as i32
}

/// Метеорологический сезон месяца (северное полушарие).
fn season_of_month(m: u32) -> Season {
    match m {
        12 | 1 | 2 => Season::Winter,
        3..=5 => Season::Spring,
        6..=8 => Season::Summer,
        _ => Season::Autumn,
    }
}

/// Веса сезонов [зима, весна, лето, осень] на дату. В окне ±`blend_days` вокруг границы
/// сезоны смешиваются линейно (0,5/0,5 ровно на границе). Южное полушарие — сдвиг на полгода.
pub fn season_weights(days: i32, southern: bool, blend_days: i32) -> [f32; 4] {
    let (y, m, _) = civil_from_days(days);
    let mut nearest = (i32::MAX, Season::Winter, 0i32);
    for yy in [y - 1, y, y + 1] {
        for (mm, starts) in [
            (3, Season::Spring),
            (6, Season::Summer),
            (9, Season::Autumn),
            (12, Season::Winter),
        ] {
            let b = days_from_civil(yy, mm, 1);
            let dist = (days - b).abs();
            if dist < nearest.0 {
                nearest = (dist, starts, b);
            }
        }
    }
    let mut w = [0.0f32; 4];
    let (dist, after, boundary) = nearest;
    if blend_days > 0 && dist < blend_days {
        let before = (after as usize + 3) % 4;
        let t = (days - (boundary - blend_days)) as f32 / (2 * blend_days) as f32;
        w[after as usize] = t;
        w[before] = 1.0 - t;
    } else {
        w[season_of_month(m) as usize] = 1.0;
    }
    if southern {
        [w[2], w[3], w[0], w[1]]
    } else {
        w
    }
}

/// Поправка погодой (не больше `cap`): например, снег в апреле добавляет вес зиме.
pub fn apply_weather_nudge(w: &mut [f32; 4], season: Season, amount: f32, cap: f32) {
    let add = amount.clamp(0.0, cap);
    w[season as usize] += add;
    let sum: f32 = w.iter().sum();
    for v in w.iter_mut() {
        *v /= sum;
    }
}

/// Средний момент равноденствия/солнцестояния (Meeus, табл. 27.B, годы 1000–3000), JDE.
/// `which`: 0 — мартовское равноденствие, 1 — июньское солнцестояние,
/// 2 — сентябрьское равноденствие, 3 — декабрьское солнцестояние.
pub fn equinox_solstice_jde(year: i32, which: u8) -> f64 {
    let y = (year as f64 - 2000.0) / 1000.0;
    let (y2, y3, y4) = (y * y, y * y * y, y * y * y * y);
    match which {
        0 => 2_451_623.809_84 + 365_242.374_04 * y + 0.051_69 * y2 - 0.004_11 * y3 - 0.000_57 * y4,
        1 => 2_451_716.567_67 + 365_241.626_03 * y + 0.003_25 * y2 + 0.008_88 * y3 - 0.000_30 * y4,
        2 => 2_451_810.217_15 + 365_242.017_67 * y - 0.115_75 * y2 + 0.003_37 * y3 + 0.000_78 * y4,
        _ => 2_451_900.059_52 + 365_242.740_49 * y - 0.062_23 * y2 - 0.008_23 * y3 + 0.000_32 * y4,
    }
}

/// UTC-дата (дни от 1970-01-01) для JDE (разницей TT−UT ≈ 70 с пренебрегаем).
pub fn jde_to_days(jde: f64) -> i32 {
    (jde - 2_440_587.5).floor() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, dd: u32) -> i32 {
        days_from_civil(y, m, dd)
    }

    #[test]
    fn civil_roundtrip_and_weekday() {
        for z in -800_000..800_000 {
            let (y, m, dd) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, dd), z);
        }
        assert_eq!(weekday(d(2026, 10, 16)), 4); // пятница
        assert_eq!(weekday(d(1970, 1, 1)), 3); // четверг
    }

    #[test]
    fn easter_2026_2030_matches_published_tables() {
        // Wikipedia «List of dates for Easter» (проверено 2026-10-16).
        let western = [
            (2026, 4, 5),
            (2027, 3, 28),
            (2028, 4, 16),
            (2029, 4, 1),
            (2030, 4, 21),
        ];
        let orthodox = [
            (2026, 4, 12),
            (2027, 5, 2),
            (2028, 4, 16),
            (2029, 4, 8),
            (2030, 4, 28),
        ];
        for (y, m, dd) in western {
            assert_eq!(easter_western(y), d(y, m, dd), "western {y}");
        }
        for (y, m, dd) in orthodox {
            assert_eq!(easter_orthodox(y), d(y, m, dd), "orthodox {y}");
        }
    }

    #[test]
    fn maslenitsa_2026_2030() {
        let expected = [
            (2026, (2, 16), (2, 22)),
            (2027, (3, 8), (3, 14)),
            (2028, (2, 21), (2, 27)),
            (2029, (2, 12), (2, 18)),
            (2030, (3, 4), (3, 10)),
        ];
        for (y, (m1, d1), (m2, d2)) in expected {
            let (start, end) = maslenitsa(y);
            assert_eq!(start, d(y, m1, d1), "start {y}");
            assert_eq!(end, d(y, m2, d2), "end {y}");
            assert_eq!(weekday(start), 0);
            assert_eq!(weekday(end), 6);
        }
    }

    #[test]
    fn season_blend_window() {
        // середина сезона — чистый сезон
        assert_eq!(
            season_weights(d(2026, 7, 15), false, 21),
            [0.0, 0.0, 1.0, 0.0]
        );
        // ровно на границе — пополам
        let b = season_weights(d(2026, 12, 1), false, 21);
        assert!((b[0] - 0.5).abs() < 1e-6 && (b[3] - 0.5).abs() < 1e-6);
        // 22 ноября (AT-14 «Предзимье»): зима уже заметна
        let w = season_weights(d(2026, 11, 22), false, 21);
        assert!(w[0] > 0.25 && w[0] < 0.35, "winter={}", w[0]);
        // 20 ноября (пример владельца «осень 0,7 / зима 0,3»): при окне 21 день ≈ 0,76/0,24
        let w20 = season_weights(d(2026, 11, 20), false, 21);
        assert!((w20[3] - 0.762).abs() < 0.01, "autumn={}", w20[3]);
        // с окном 10 дней (буквально по дополнению) 20 ноября — ещё чистая осень
        assert_eq!(
            season_weights(d(2026, 11, 20), false, 10),
            [0.0, 0.0, 0.0, 1.0]
        );
        // южное полушарие: январь — лето
        assert_eq!(
            season_weights(d(2027, 1, 15), true, 21),
            [0.0, 0.0, 1.0, 0.0]
        );
        // сумма весов всегда 1
        for z in d(2026, 1, 1)..d(2028, 1, 1) {
            let s: f32 = season_weights(z, false, 21).iter().sum();
            assert!((s - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn weather_nudge_is_capped() {
        let mut w = [0.0, 1.0, 0.0, 0.0]; // апрель, весна
        apply_weather_nudge(&mut w, Season::Winter, 5.0, 0.3);
        assert!((w[0] - 0.3 / 1.3).abs() < 1e-6);
    }

    /// Wikipedia «Solstice», table «UT date and time of equinoxes and solstices on Earth»
    /// (checked 2026-10-17). Minutes: the mean formulas must land within 20 min.
    /// (month, day, hour, minute) UTC of the four moments of a year.
    type Moments = [(u32, u32, u32, u32); 4];
    const ASTRO: [(i32, Moments); 5] = [
        (
            2026,
            [
                (3, 20, 14, 46),
                (6, 21, 8, 25),
                (9, 23, 0, 6),
                (12, 21, 20, 50),
            ],
        ),
        (
            2027,
            [
                (3, 20, 20, 25),
                (6, 21, 14, 11),
                (9, 23, 6, 2),
                (12, 22, 2, 43),
            ],
        ),
        (
            2028,
            [
                (3, 20, 2, 17),
                (6, 20, 20, 2),
                (9, 22, 11, 45),
                (12, 21, 8, 20),
            ],
        ),
        (
            2029,
            [
                (3, 20, 8, 1),
                (6, 21, 1, 48),
                (9, 22, 17, 37),
                (12, 21, 14, 14),
            ],
        ),
        (
            2030,
            [
                (3, 20, 13, 51),
                (6, 21, 7, 31),
                (9, 22, 23, 27),
                (12, 21, 20, 9),
            ],
        ),
    ];

    #[test]
    fn equinoxes_and_solstices_2026_2030() {
        for (y, events) in ASTRO {
            for (which, (m, dd, h, min)) in events.into_iter().enumerate() {
                let want = i64::from(d(y, m, dd)) * 86_400_000 + i64::from(h * 60 + min) * 60_000;
                let got = astro_utc_ms(y, which as u8);
                assert!(
                    (got - want).abs() <= 20 * 60_000,
                    "{y} event {which}: {} min off",
                    (got - want) / 60_000
                );
            }
        }
    }

    #[test]
    fn astro_day_is_local() {
        // December solstice 2026: 21.12 20:50 UTC = 23:50 in Moscow, still the 21st
        assert_eq!(astro_local_day(2026, 3, 0), d(2026, 12, 21));
        assert_eq!(astro_local_day(2026, 3, 180), d(2026, 12, 21));
        // Vladivostok (+10): 22.12 06:50
        assert_eq!(astro_local_day(2026, 3, 600), d(2026, 12, 22));
        // September equinox 2026 at 00:06 UTC is still 22.09 in New York (−4)
        assert_eq!(astro_local_day(2026, 2, -240), d(2026, 9, 22));
    }

    #[test]
    fn nth_weekday_rules() {
        // Mother's Day RU: last Sunday of November 2026 = 29.11
        assert_eq!(nth_weekday(2026, 11, 6, -1), Some(d(2026, 11, 29)));
        // 2nd Sunday of May 2027 = 9.05
        assert_eq!(nth_weekday(2027, 5, 6, 2), Some(d(2027, 5, 9)));
        // 1st Monday of June 2026 = 1.06 (the month starts on Monday)
        assert_eq!(nth_weekday(2026, 6, 0, 1), Some(d(2026, 6, 1)));
        // last Friday of February 2028 (leap year) = 25.02
        assert_eq!(nth_weekday(2028, 2, 4, -1), Some(d(2028, 2, 25)));
        // there is no 5th Monday in February 2027
        assert_eq!(nth_weekday(2027, 2, 0, 5), None);
        assert_eq!(nth_weekday(2027, 2, 0, 0), None);
        assert_eq!(days_in_month(2028, 2), 29);
        assert_eq!(days_in_month(2027, 2), 28);
    }
}
