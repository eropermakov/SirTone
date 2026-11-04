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

/// `sirtone-cli wave coverage <library.db> [days]` — which tracks «Моя волна» reaches (T7.9): the
/// real engine over a library and its journal, listening `days` (3) days around the clock, each
/// pick heard to the end. Prints the tracks never picked with what the engine knows of them.
/// Run it on a COPY of `library.db` (it is opened writable: migrations go forward).
pub fn coverage(db: &str, days: &str) -> ExitCode {
    use sirtone_library::wave as w;
    use sirtone_wave::engine::{Config, Engine};
    let days: i64 = days.parse().unwrap_or(3);
    let conn = match sirtone_library::db::open_existing(
        std::path::Path::new(db),
        sirtone_library::Medium::Fixed,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("wave coverage: {e}");
            return ExitCode::FAILURE;
        }
    };
    let clock = SystemClock;
    let start = clock.now_utc_ms();
    let read = || -> Result<_, sirtone_library::LibraryError> {
        let history = sirtone_wave::learn::History {
            events: w::plays_since(&conn, 0)?,
            feedback: w::feedback_since(&conn, 0)?,
        };
        Ok((w::catalog(&conn)?, w::not_now(&conn, start)?, history))
    };
    let (tracks, shelf, history) = match read() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("wave coverage: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Ok(cfg) = Config::builtin_profile("my_wave") else {
        eprintln!("wave coverage: no profile");
        return ExitCode::FAILURE;
    };
    let mut e = Engine::new(cfg, &tracks, &history, start, 7);
    let ctxb = sirtone_wave::context::ContextBuilder::default();
    // Familiarity groups at the start (§9: 0 loved/familiar, 1 forgotten, 2 new/untouched), the
    // unanalysed apart.
    let mut groups = [[0u32; 3]; 2];
    for t in &tracks {
        if let Some(g) = e.group_of(t.id, start) {
            groups[usize::from(t.features.is_none())][g] += 1;
        }
    }
    println!(
        "groups (familiar/forgotten/new): analysed {:?}, not analysed {:?}",
        groups[0], groups[1]
    );
    let mut picks = std::collections::HashMap::<i64, u32>::new();
    let mut first = std::collections::HashMap::<i64, u32>::new();
    let (mut now, end, mut n) = (start, start + days * 86_400_000, 0u32);
    while now < end {
        let ctx = ctxb.snapshot_at(&clock, now);
        let Some(&id) = e.next(&ctx, &shelf, 0.5, 1, now).first() else {
            println!("the wave ran dry after {n} tracks");
            break;
        };
        if n == 0 {
            for (label, analysed) in [("analysed", true), ("not analysed", false)] {
                let mut sum = [0f32; 6];
                let mut k = 0f32;
                for t in tracks.iter().filter(|t| t.features.is_some() == analysed) {
                    if let Some(p) = e.parts(t.id) {
                        for (s, v) in sum.iter_mut().zip(p) {
                            *s += v;
                        }
                        k += 1.0;
                    }
                }
                let m: Vec<String> = sum
                    .iter()
                    .map(|v| format!("{:.3}", v / k.max(1.0)))
                    .collect();
                println!(
                    "{label} mean parts (taste, context, session, season, beta mean, cold): {}",
                    m.join(", ")
                );
            }
        }
        *picks.entry(id).or_default() += 1;
        n += 1;
        first.entry(id).or_insert(n);
        if [20, 50, 100, 200, 400].contains(&n) {
            println!(
                "after {n} picks ({:.1} h): {} distinct",
                (now - start) as f64 / 3.6e6,
                picks.len()
            );
        }
        e.played(id, now);
        let dur = tracks
            .iter()
            .find(|t| t.id == id)
            .map_or(180_000, |t| t.duration_ms);
        now += dur.max(30_000) as i64;
    }
    let cat = e.catalog();
    println!(
        "{} tracks in the library, {} in the engine, {n} picks over {days} days, {} distinct",
        tracks.len(),
        cat.len(),
        picks.len()
    );
    let mut most: Vec<_> = picks.iter().collect();
    most.sort_by(|a, b| b.1.cmp(a.1));
    let top: Vec<String> = most
        .iter()
        .take(5)
        .map(|(id, c)| format!("#{id}×{c}"))
        .collect();
    println!("most picked: {}", top.join(", "));
    let fewest: Vec<String> = most
        .iter()
        .rev()
        .take(8)
        .map(|(id, c)| format!("#{id}×{c}"))
        .collect();
    println!("least picked: {}", fewest.join(", "));
    // When each kind of track is first heard: the analysed and the unanalysed.
    for (label, analysed) in [("analysed", true), ("not analysed", false)] {
        let mut at: Vec<u32> = tracks
            .iter()
            .filter(|t| t.features.is_some() == analysed)
            .filter_map(|t| first.get(&t.id).copied())
            .collect();
        at.sort_unstable();
        if let (Some(lo), Some(hi)) = (at.first(), at.last()) {
            println!(
                "{label}: {} tracks, first heard at pick {lo}…{hi}, median {}",
                at.len(),
                at[at.len() / 2]
            );
        }
    }
    println!("\nnever picked:");
    for t in &tracks {
        if picks.contains_key(&t.id) {
            continue;
        }
        let i = cat.idx(t.id);
        println!(
            "  #{} «{}» — {}: analysed {}, plays {}, skips {}, completion {:.2}, mean {}, never {}, no_season {}, group {:?}, genre {:?}",
            t.id,
            t.title.as_deref().unwrap_or("?"),
            t.artist_name.as_deref().unwrap_or("?"),
            t.features
                .map_or("no".to_string(), |f| format!("{:.2}", f.1)),
            t.play_count,
            t.skip_count,
            t.completion,
            i.map_or("—".to_string(), |i| format!("{:.2}", cat.mean(i))),
            t.never,
            i.is_some_and(|i| cat.no_season[i]),
            e.group_of(t.id, now),
            t.genre,
        );
    }
    ExitCode::SUCCESS
}
