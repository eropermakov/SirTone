//! Track seasonality (T3.30, docs/14 §10.2) and the seasonal card name (§10.3).
//!
//! `aff(t, s)` ∈ [0, 1] for [winter, spring, summer, autumn], 0.5 = says nothing:
//! a manual tag decides (1 for its season, 0 for the rest; «убрать из сезонной» — 0.5 here and
//! out of the seasonal wave); otherwise sound · words · history,
//! each weighted — words 0.3 when a season word is found (title 1.0 · album 0.7 · artist 0.3),
//! history up to 0.5 after a year of listening, sound the rest but × analysis confidence, so a
//! track without deep analysis is never given a season by library means. No source → 0.5.
//! Words are matched once per track and cached by the engine (refills reuse them).

use serde::Deserialize;
use sirtone_core::text::norm;
use sirtone_core::wave::{FEATURES as D, PlayEvent};

const DAY_MS: i64 = 24 * 3600 * 1000;

/// Manual tag bits (`track_tag`): 1 << season for `season_*`, `NO_SEASON` for «убрать из
/// сезонной».
pub const NO_SEASON: u8 = 1 << 4;

/// Season shares of a track's earlier plays and how many there were.
pub type Plays = ([f32; 4], u32);

#[derive(Debug, Clone, Deserialize)]
struct Fields {
    title: f32,
    album: f32,
    artist: f32,
}

#[derive(Debug, Clone, Deserialize)]
struct Criterion {
    f: String,
    min: Option<f32>,
    max: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
struct SoundJson {
    winter: Vec<Criterion>,
    spring: Vec<Criterion>,
    summer: Vec<Criterion>,
    autumn: Vec<Criterion>,
}

#[derive(Debug, Clone, Deserialize)]
struct AffJson {
    words_weight: f32,
    words_fields: Fields,
    history_max: f32,
    history_full_days: f32,
    history_min_plays: u32,
    history_skip_days: i64,
    tol: f32,
    sound: SoundJson,
}

#[derive(Debug, Clone, Deserialize)]
struct DictJson {
    schema: String,
    seasons: DictSeasons,
}

#[derive(Debug, Clone, Deserialize)]
struct DictSeasons {
    winter: Vec<String>,
    spring: Vec<String>,
    summer: Vec<String>,
    autumn: Vec<String>,
}

/// One dictionary entry: words in a row; the last may be a prefix («зимн*»).
#[derive(Debug, Clone)]
struct Entry {
    words: Vec<String>,
    prefix: bool,
}

impl Entry {
    fn parse(s: &str) -> Option<Self> {
        let prefix = s.ends_with('*');
        let words: Vec<String> = norm(s.trim_end_matches('*'))?
            .split(' ')
            .map(str::to_owned)
            .collect();
        Some(Self { words, prefix })
    }

    fn matches(&self, tokens: &[&str]) -> bool {
        let n = self.words.len();
        tokens.windows(n).any(|w| {
            w.iter().zip(&self.words).enumerate().all(|(i, (t, e))| {
                if self.prefix && i == n - 1 {
                    t.starts_with(e.as_str())
                } else {
                    t == e
                }
            })
        })
    }
}

/// A sound target on one feature (§4.1 names).
#[derive(Debug, Clone, Copy)]
struct Target {
    f: usize,
    min: f32,
    max: f32,
}

fn feature(name: &str) -> Option<usize> {
    Some(match name {
        "energy" => 0,
        "valence" => 1,
        "arousal" => 2,
        "danceability" => 3,
        "tempo" => 4,
        "mode" => 7,
        "acousticness" => 8,
        "vocal" => 9,
        "brightness" => 10,
        "dynamics" => 11,
        "loudness" => 12,
        "aggression" => 15,
        "festive" => 25,
        _ => return None,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum SeasonError {
    #[error("season config: {0}")]
    Json(#[from] serde_json::Error),
    #[error("season config: {0}")]
    Invalid(String),
}

/// Seasonality rules: dictionaries RU/EN, source weights, sound profiles.
#[derive(Debug, Clone)]
pub struct Seasonality {
    dict: [Vec<Entry>; 4],
    words_weight: f32,
    fields: [f32; 3],
    history_max: f32,
    history_full_days: f32,
    history_min_plays: u32,
    history_skip_days: i64,
    tol: f32,
    sound: [Vec<Target>; 4],
}

impl Seasonality {
    /// The shipped `seasons.json` and `dict/{ru,en}.json`.
    pub fn builtin() -> Result<Self, SeasonError> {
        Self::from_json(
            include_str!("../../../assets/wave/seasons.json"),
            &[
                include_str!("../../../assets/wave/dict/ru.json"),
                include_str!("../../../assets/wave/dict/en.json"),
            ],
        )
    }

    pub fn from_json(seasons: &str, dicts: &[&str]) -> Result<Self, SeasonError> {
        let a: AffJson = serde_json::from_str(seasons)?;
        let mut dict: [Vec<Entry>; 4] = Default::default();
        for text in dicts {
            let d: DictJson = serde_json::from_str(text)?;
            if d.schema != "sirtone.dict/1" {
                return Err(SeasonError::Invalid(d.schema));
            }
            let s = d.seasons;
            for (i, words) in [s.winter, s.spring, s.summer, s.autumn].iter().enumerate() {
                dict[i].extend(words.iter().filter_map(|w| Entry::parse(w)));
            }
        }
        let targets = |v: &[Criterion]| -> Result<Vec<Target>, SeasonError> {
            v.iter()
                .map(|c| {
                    let f = feature(&c.f).ok_or_else(|| SeasonError::Invalid(c.f.clone()))?;
                    Ok(Target {
                        f,
                        min: c.min.unwrap_or(0.0),
                        max: c.max.unwrap_or(1.0),
                    })
                })
                .collect()
        };
        let s = &a.sound;
        Ok(Self {
            dict,
            words_weight: a.words_weight,
            fields: [
                a.words_fields.title,
                a.words_fields.album,
                a.words_fields.artist,
            ],
            history_max: a.history_max,
            history_full_days: a.history_full_days.max(1.0),
            history_min_plays: a.history_min_plays,
            history_skip_days: a.history_skip_days,
            tol: a.tol.max(1e-3),
            sound: [
                targets(&s.winter)?,
                targets(&s.spring)?,
                targets(&s.summer)?,
                targets(&s.autumn)?,
            ],
        })
    }

    /// Season words in the title, album and artist: per season the strongest field weight
    /// (0 — not found). Title outweighs the artist (an artist named «Снег» is weak evidence).
    pub fn words(
        &self,
        title: Option<&str>,
        album: Option<&str>,
        artist: Option<&str>,
    ) -> [f32; 4] {
        let mut m = [0f32; 4];
        for (text, w) in [title, album, artist].into_iter().zip(self.fields) {
            let Some(n) = text.and_then(norm) else {
                continue;
            };
            let tokens: Vec<&str> = n
                .split(|c: char| !(c.is_alphanumeric() || c == '\''))
                .filter(|t| !t.is_empty())
                .collect();
            for (s, entries) in self.dict.iter().enumerate() {
                if m[s] < w && entries.iter().any(|e| e.matches(&tokens)) {
                    m[s] = w;
                }
            }
        }
        m
    }

    /// Sound fit per season, 0…1 (mean over the profile's targets).
    fn sound(&self, x: &[f32; D]) -> [f32; 4] {
        std::array::from_fn(|s| {
            let t = &self.sound[s];
            if t.is_empty() {
                return 0.5;
            }
            let sum: f32 = t
                .iter()
                .map(|c| {
                    let v = x[c.f];
                    let miss = (c.min - v).max(v - c.max).max(0.0);
                    (1.0 - miss / self.tol).max(0.0)
                })
                .sum();
            sum / t.len() as f32
        })
    }

    /// The final affinity of a track (§10.2).
    /// `words` — from [`Self::words`]; `hist` — season shares of its earlier plays and how many;
    /// `history_days` — how long the journal is; `features` — the vector and its confidence.
    pub fn affinity(
        &self,
        tags: u8,
        words: &[f32; 4],
        hist: Option<Plays>,
        history_days: f32,
        features: Option<(&[f32; D], f32)>,
    ) -> [f32; 4] {
        if tags & NO_SEASON != 0 {
            return [0.5; 4];
        }
        if tags & 0x0f != 0 {
            return std::array::from_fn(|s| f32::from(u8::from(tags & (1 << s) != 0)));
        }
        let mmax = words.iter().copied().fold(0.0, f32::max);
        let w_words = if mmax > 0.0 { self.words_weight } else { 0.0 };
        let w_hist = match hist {
            Some((_, n)) if n >= self.history_min_plays => {
                self.history_max * (history_days / self.history_full_days).min(1.0)
            }
            _ => 0.0,
        };
        let w_sound = features.map_or(0.0, |(_, conf)| {
            (1.0 - w_words - w_hist).max(0.0) * conf.clamp(0.0, 1.0)
        });
        let total = w_words + w_hist + w_sound;
        if total <= 1e-6 {
            return [0.5; 4];
        }
        let sound = features.map_or([0.5; 4], |(x, _)| self.sound(x));
        let shares = hist.map_or([0.25; 4], |(h, _)| h);
        std::array::from_fn(|s| {
            // Words: the matched season up, the others down by the strongest match.
            let words_s = if words[s] > 0.0 {
                0.5 + 0.5 * words[s]
            } else {
                0.5 - 0.5 * mmax
            };
            // History: a quarter of the plays is «even» (0.5), half or more — fully this season.
            let hist_s = (2.0 * shares[s]).min(1.0);
            (w_words * words_s + w_hist * hist_s + w_sound * sound[s]) / total
        })
    }

    /// Season shares of each track's earlier plays: plays heard ≥ 30 s, older than the skip
    /// window (so this season's listening does not feed itself), weighted by the season blend
    /// of that day. Returns shares and play counts by catalog index, and the journal's age in days.
    pub fn history(
        &self,
        events: &[PlayEvent],
        index: impl Fn(i64) -> Option<usize>,
        n: usize,
        now_ms: i64,
    ) -> (Vec<Option<Plays>>, f32) {
        let mut acc: Vec<Plays> = vec![([0.0; 4], 0); n];
        let cut = now_ms - self.history_skip_days * DAY_MS;
        let mut first = now_ms;
        for e in events {
            first = first.min(e.ts);
            if e.ts >= cut || e.played_ms < 30_000 {
                continue;
            }
            if let Some(i) = index(e.track_id) {
                let a = &mut acc[i];
                for (v, w) in a.0.iter_mut().zip(e.context.season_w) {
                    *v += w;
                }
                a.1 += 1;
            }
        }
        let days = ((now_ms - first) / DAY_MS) as f32;
        let out = acc
            .into_iter()
            .map(|(sum, k)| {
                (k > 0).then(|| {
                    let total: f32 = sum.iter().sum::<f32>().max(1e-6);
                    (sum.map(|v| v / total), k)
                })
            })
            .collect();
        (out, days)
    }
}

/// The seasonal card's name key (§10.3): in a transition window the mix names it
/// («Предзимье», «Начало зимы»…), outside — the season («Осенняя волна»).
/// Keys: `winter|spring|summer|autumn` or `<lateX>|<earlyY>` as in `wave.season.*`.
pub fn card_key(w: &[f32; 4]) -> &'static str {
    const PURE: [&str; 4] = ["winter", "spring", "summer", "autumn"];
    // Before the boundary of season → next (the dominant season ends).
    const LATE: [&str; 4] = ["lateWinter", "lateSpring", "lateSummer", "preWinter"];
    // After it (the dominant season has just begun).
    const EARLY: [&str; 4] = ["earlyWinter", "earlySpring", "earlySummer", "earlyAutumn"];
    let mut order = [0usize, 1, 2, 3];
    order.sort_by(|&a, &b| w[b].total_cmp(&w[a]));
    let (main, second) = (order[0], order[1]);
    if w[second] < 0.05 {
        return PURE[main];
    }
    if second == (main + 1) % 4 {
        LATE[main]
    } else {
        EARLY[main]
    }
}

/// The dominant season (0 winter … 3 autumn) of the weights.
pub fn dominant(w: &[f32; 4]) -> usize {
    (0..4).max_by(|&a, &b| w[a].total_cmp(&w[b])).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::season_weights;

    fn d(y: i32, m: u32, day: u32) -> i32 {
        crate::calendar::days_from_civil(y, m, day)
    }

    #[test]
    fn card_names_on_both_sides_of_every_boundary() {
        let k = |y, m, day, south| card_key(&season_weights(d(y, m, day), south, 21));
        // The acceptance example: 22 November → «Предзимье».
        assert_eq!(k(2026, 11, 22, false), "preWinter");
        assert_eq!(k(2026, 12, 10, false), "earlyWinter");
        assert_eq!(k(2027, 1, 20, false), "winter");
        assert_eq!(k(2027, 2, 20, false), "lateWinter");
        assert_eq!(k(2027, 3, 10, false), "earlySpring");
        assert_eq!(k(2027, 4, 15, false), "spring");
        assert_eq!(k(2027, 5, 20, false), "lateSpring");
        assert_eq!(k(2027, 6, 10, false), "earlySummer");
        assert_eq!(k(2027, 7, 15, false), "summer");
        assert_eq!(k(2027, 8, 20, false), "lateSummer");
        assert_eq!(k(2027, 9, 10, false), "earlyAutumn");
        assert_eq!(k(2027, 10, 15, false), "autumn");
        // Southern hemisphere: half a year apart.
        assert_eq!(k(2027, 1, 15, true), "summer");
        assert_eq!(k(2027, 5, 22, true), "preWinter");
    }

    fn s() -> Seasonality {
        Seasonality::builtin().unwrap()
    }

    #[test]
    fn tags_decide() {
        let s = s();
        let words = s.words(Some("Летний дождь"), None, None);
        assert_eq!(
            s.affinity(1 << 0, &words, None, 0.0, None),
            [1.0, 0.0, 0.0, 0.0]
        );
        assert_eq!(s.affinity(NO_SEASON, &words, None, 0.0, None), [0.5; 4]);
    }

    #[test]
    fn no_source_says_nothing_and_means_are_not_a_season() {
        let s = s();
        let w = s.words(Some("Song"), Some("Album"), Some("Band"));
        assert_eq!(s.affinity(0, &w, None, 400.0, None), [0.5; 4]);
        // Unanalysed (conf 0): the sound profiles do not count.
        let x = [0.5; D];
        assert_eq!(s.affinity(0, &w, None, 400.0, Some((&x, 0.0))), [0.5; 4]);
    }

    #[test]
    fn title_outweighs_artist() {
        let s = s();
        let title = s.affinity(0, &s.words(Some("Снег идёт"), None, None), None, 0.0, None);
        let artist = s.affinity(
            0,
            &s.words(Some("Песня"), None, Some("Снег")),
            None,
            0.0,
            None,
        );
        assert_eq!(title[0], 1.0);
        assert!(artist[0] > 0.5 && artist[0] < title[0], "{artist:?}");
        assert!(title[2] < artist[2]);
    }

    #[test]
    fn history_grows_to_half_in_a_year() {
        let s = s();
        let w = [0.0; 4];
        let summer = Some(([0.0, 0.0, 1.0, 0.0], 10));
        let young = s.affinity(0, &w, summer, 30.0, None);
        let year = s.affinity(0, &w, summer, 400.0, None);
        assert_eq!(young[2], 1.0); // the only source
        assert_eq!(year[2], 1.0);
        // With words for another season the history weighs more after a year.
        let ww = s.words(Some("Winter"), None, None);
        let a = s.affinity(0, &ww, summer, 30.0, None);
        let b = s.affinity(0, &ww, summer, 400.0, None);
        assert!(b[2] > a[2], "{a:?} {b:?}");
        // Too few plays — no history.
        assert_eq!(
            s.affinity(0, &w, Some(([0.0, 0.0, 1.0, 0.0], 2)), 400.0, None),
            [0.5; 4]
        );
    }

    #[test]
    fn sound_counts_by_confidence() {
        let s = s();
        let mut x = [0.5f32; D];
        x[0] = 0.9; // energy
        x[1] = 0.9; // valence
        x[3] = 0.9; // danceability
        x[10] = 0.9; // brightness
        x[7] = 1.0; // major
        let a = s.affinity(0, &[0.0; 4], None, 0.0, Some((&x, 1.0)));
        assert!(a[2] > 0.9 && a[0] < 0.5, "{a:?}");
    }

    /// Dictionary set (docs/05 §13): RU/EN titles with the expected season (None — no season),
    /// incl. false friends. The full 300-title set needs the owner's golden set (OWN-08).
    #[test]
    fn dictionary_precision() {
        let s = s();
        let cases: &[(&str, Option<usize>)] = &[
            ("Зима", Some(0)),
            ("Зимний вечер", Some(0)),
            ("Снег кружится", Some(0)),
            ("Снежинка", Some(0)),
            ("Метель", Some(0)),
            ("Новогодняя", Some(0)),
            ("Три белых коня (декабрь)", Some(0)),
            ("Последний день января", Some(0)),
            ("Let It Snow!", Some(0)),
            ("Winter Wonderland", Some(0)),
            ("Last Christmas", Some(0)),
            ("Frosty the Snowman", Some(0)),
            ("December", Some(0)),
            ("Весна", Some(1)),
            ("Весенняя", Some(1)),
            ("Апрель", Some(1)),
            ("Восьмое марта", Some(1)),
            ("Майский вальс", Some(1)),
            ("Капель", Some(1)),
            ("Spring Day", Some(1)),
            ("April Come She Will", Some(1)),
            ("Лето", Some(2)),
            ("Летний дождь", Some(2)),
            ("Июнь", Some(2)),
            ("Жара", Some(2)),
            ("Пляж", Some(2)),
            ("Summertime", Some(2)),
            ("Summer of '69", Some(2)),
            ("Cruel Summer", Some(2)),
            ("Осень", Some(3)),
            ("Осенняя", Some(3)),
            ("Что такое осень", Some(3)),
            ("Сентябрь горит", Some(3)),
            ("Листопад", Some(3)),
            ("Autumn Leaves", Some(3)),
            ("Wake Me Up When September Ends", Some(3)),
            ("November Rain", Some(3)),
            ("October", Some(3)),
            // False friends: not a season.
            ("Летать", None),
            ("Улетай", None),
            ("Полёт", None),
            ("Лётчик", None),
            ("Маяк", None),
            ("Майор", None),
            ("Марта нет", None), // a known miss: the name reads as «марта»
            ("Мартин", None),
            ("Сирена", None),
            ("Жаркое из курицы", None), // a known miss: «жарк*» reads it as summer
            ("Holy Ghost", None),
            ("Springsteen", None),
            ("Fall Out", None),
            ("March of the Pigs", None),
            ("May I", None),
            ("Somewhere", None),
            ("Ice Ice Baby", None),
            ("Cold as Ice", None),
            ("Rain", None),
            ("Дождь", None),
            ("Холодно", None),
            ("Augustana", None),
            ("Juneau", None),
            ("Летов", None),
            ("Летние", Some(2)),
        ];
        let mut ok = 0;
        let mut bad = Vec::new();
        for (title, want) in cases {
            let m = s.words(Some(title), None, None);
            let got = (0..4).find(|&i| m[i] > 0.0);
            if got == *want {
                ok += 1;
            } else {
                bad.push((*title, got));
            }
        }
        let precision = ok as f32 / cases.len() as f32;
        assert!(precision >= 0.95, "{precision} {bad:?}");
    }

    #[test]
    fn history_skips_recent_and_short_plays() {
        use sirtone_core::wave::{ContextSnapshot, Daypart, EndReason, PlaySource};
        let s = s();
        let now = 1_800_000_000_000;
        let ev = |ts: i64, played: u64, w: [f32; 4]| PlayEvent {
            ts,
            track_id: 7,
            played_ms: played,
            duration_ms: 200_000,
            end_reason: EndReason::Finished,
            source: PlaySource::Wave,
            profile: None,
            context: ContextSnapshot {
                now_utc_ms: ts,
                local_minute: 0,
                weekday: 0,
                daypart: Daypart::Day,
                season_w: w,
                weather: None,
            },
            event_id: None,
            novelty: None,
            category: None,
        };
        let summer = [0.0, 0.0, 1.0, 0.0];
        let events = vec![
            ev(now - 300 * DAY_MS, 180_000, summer),
            ev(now - 290 * DAY_MS, 180_000, summer),
            ev(now - 280 * DAY_MS, 10_000, [1.0, 0.0, 0.0, 0.0]), // too short
            ev(now - DAY_MS, 180_000, [1.0, 0.0, 0.0, 0.0]),      // this season
        ];
        let (h, days) = s.history(&events, |id| (id == 7).then_some(0), 1, now);
        let (shares, n) = h[0].unwrap();
        assert_eq!(n, 2);
        assert_eq!(shares, summer);
        assert_eq!(days, 300.0);
    }
}
