//! `sirtone-cli wave calendar <year>` — the offline calendar of the wave engine (T1.29): season
//! boundaries with transition windows, Easter, Maslenitsa, equinoxes and solstices in local time.

use std::process::ExitCode;

use sirtone_core::wave::Clock;
use sirtone_platform::clock::SystemClock;
use sirtone_wave::calendar::{
    astro_local_day, astro_utc_ms, civil_from_days, days_from_civil, easter_orthodox,
    easter_western, maslenitsa, season_weights, weekday,
};
use sirtone_wave::config::SeasonsConfig;

const WD: [&str; 7] = ["пн", "вт", "ср", "чт", "пт", "сб", "вс"];
const SEASONS: [&str; 4] = ["зима", "весна", "лето", "осень"];

fn date(day: i32) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{d:02}.{m:02}.{y} {}", WD[weekday(day) as usize])
}

pub fn calendar(year: &str) -> ExitCode {
    let Ok(y) = year.parse::<i32>() else {
        eprintln!("wave calendar: year expected");
        return ExitCode::from(2);
    };
    let cfg = SeasonsConfig::default();
    let blend = cfg.blend_days;
    println!("Календарь волны {y} (северное полушарие, окно перехода ±{blend} дн.)\n");
    println!("Сезоны:");
    for (m, s) in [(3, 1usize), (6, 2), (9, 3), (12, 0)] {
        let b = days_from_civil(y, m, 1);
        let prev = (s + 3) % 4;
        println!(
            "  {} → {}: граница {}, переход {} … {}",
            SEASONS[prev],
            SEASONS[s],
            date(b),
            date(b - blend),
            date(b + blend - 1)
        );
    }
    println!("\nВеса сезонов по месяцам (1-е и 15-е число):");
    for m in 1..=12 {
        for d in [1, 15] {
            let w = season_weights(days_from_civil(y, m, d), false, blend);
            let parts: Vec<String> = (0..4)
                .filter(|&i| w[i] > 0.0)
                .map(|i| format!("{} {:.2}", SEASONS[i], w[i]))
                .collect();
            println!("  {d:02}.{m:02}: {}", parts.join(", "));
        }
    }
    let (ms, me) = maslenitsa(y);
    println!("\nДаты:");
    println!("  Пасха (западная):      {}", date(easter_western(y)));
    println!("  Пасха (православная):  {}", date(easter_orthodox(y)));
    println!("  Масленица:             {} … {}", date(ms), date(me));
    let clock = SystemClock;
    let names = [
        "Мартовское равноденствие",
        "Июньское солнцестояние",
        "Сентябрьское равноденствие",
        "Декабрьское солнцестояние",
    ];
    for (i, n) in names.iter().enumerate() {
        let utc = astro_utc_ms(y, i as u8);
        let off = clock.local_offset_min(utc);
        let local = utc + i64::from(off) * 60_000;
        let min = local.rem_euclid(86_400_000) / 60_000;
        println!(
            "  {n:<27} {} {:02}:{:02} (UTC{:+})",
            date(astro_local_day(y, i as u8, off)),
            min / 60,
            min % 60,
            off as f32 / 60.0
        );
    }
    ExitCode::SUCCESS
}
