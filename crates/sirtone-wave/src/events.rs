//! Events (feature C, T5.13, FR-EVT-001…003, docs/14 §11, supplement §29): the offline calendar
//! (`assets/wave/events.json`) — every rule kind of §11.1 — and how well a track suits an event
//! (§11.2). Pure: the caller passes the local date and time from `Clock` and the weather flags.

use serde::Deserialize;
use sirtone_core::text::norm;
use sirtone_core::wave::{FEATURES as D, WaveTrack};

use crate::calendar::{
    astro_local_day, days_from_civil, easter_orthodox, easter_western, nth_weekday, weekday,
};
use crate::config::ConfigError;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Rule {
    Fixed {
        month: u32,
        day: u32,
    },
    /// `base`: `easter_western` · `easter_orthodox`; + `offset` days.
    Computed {
        base: String,
        offset: i32,
    },
    /// `weekday` 0 = Monday; `n = -1` — the last.
    NthWeekday {
        month: u32,
        weekday: u32,
        n: i32,
    },
    /// `first_snowfall_of_season` · `first_warm_day` · `thunderstorm` — the caller's flags.
    Weather {
        trigger: String,
    },
    Weekly {
        weekday: u32,
        from: String,
        to: String,
    },
    /// `march_equinox` · `june_solstice` · `september_equinox` · `december_solstice` ·
    /// `longest_night` (the December solstice in the north, June in the south).
    Astro {
        which: String,
    },
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct Window {
    pub start: i32,
    pub peak: i32,
    pub end: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Tone {
    #[serde(default)]
    pub moods: Vec<String>,
    #[serde(default)]
    pub exclude_moods: Vec<String>,
    #[serde(default)]
    pub minor: bool,
    #[serde(default)]
    pub valence: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventDef {
    pub id: String,
    /// `secular` · `religious` · `folk` · `micro_weather` · `micro_weekly` · `micro_astro`
    pub group: String,
    pub rule: Rule,
    #[serde(default)]
    pub window: Window,
    #[serde(default)]
    pub tone: Tone,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub genre_bonus: std::collections::BTreeMap<String, f32>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub effect: Option<String>,
    #[serde(default = "normal")]
    pub suggest_strength: String,
    #[serde(default)]
    pub default_on: bool,
    /// The auto mode may start it without a card (9 May — never, Q-2).
    #[serde(default = "yes")]
    pub auto: bool,
}

fn normal() -> String {
    "normal".into()
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventsConfig {
    schema: String,
    pub events: Vec<EventDef>,
}

const MOODS: [&str; 8] = [
    "happy",
    "upbeat",
    "calm",
    "sad",
    "aggressive",
    "tender",
    "gloomy",
    "festive",
];
/// Feature index of the first mood (docs/14 §4.1: 18–25) and of the mode (7).
const MOOD0: usize = 18;
const MODE: usize = 7;
const VALENCE: usize = 1;

impl EventsConfig {
    pub fn from_json(s: &str) -> Result<Self, ConfigError> {
        let c: Self = serde_json::from_str(s)?;
        if c.schema != "sirtone.events/1" {
            return Err(ConfigError::Schema(c.schema));
        }
        for e in &c.events {
            let w = e.window;
            let bad_mood = e
                .tone
                .moods
                .iter()
                .chain(&e.tone.exclude_moods)
                .any(|m| !MOODS.contains(&m.as_str()));
            if w.start > w.peak || w.peak > w.end || bad_mood {
                return Err(ConfigError::Invalid(format!("event {}", e.id)));
            }
            if let Rule::Weekly { from, to, .. } = &e.rule
                && (hhmm(from).is_none() || hhmm(to).is_none())
            {
                return Err(ConfigError::Invalid(format!("event {} times", e.id)));
            }
        }
        Ok(c)
    }

    pub fn get(&self, id: &str) -> Option<&EventDef> {
        self.events.iter().find(|e| e.id == id)
    }
}

impl Default for EventsConfig {
    // The shipped file is compiled in and checked by `shipped_events_parse`.
    #[allow(clippy::expect_used)]
    fn default() -> Self {
        Self::from_json(include_str!("../../../assets/wave/events.json"))
            .expect("shipped events.json is valid (test)")
    }
}

fn hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// The weather's micro-event triggers on this day (the app keeps «once per season/year»).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WeatherFlags {
    pub first_snow: bool,
    pub first_warm: bool,
    pub storm: bool,
}

/// Where «now» is for the calendar.
#[derive(Debug, Clone, Copy)]
pub struct When {
    /// Local days since 1970-01-01.
    pub day: i32,
    /// Local minutes since midnight.
    pub minute: u16,
    pub offset_min: i32,
    pub southern: bool,
    pub weather: WeatherFlags,
}

/// An event in its window: 0…1 strength (1 at the peak), days to the peak (< 0 — past it).
#[derive(Debug, Clone, PartialEq)]
pub struct Active {
    pub id: String,
    pub intensity: f32,
    pub days_to_peak: i32,
    /// The first day of this occurrence (a year's key for «не в этом году»).
    pub anchor_year: i32,
}

/// The rule's date (local days) for a year.
fn anchor(e: &EventDef, year: i32, southern: bool, offset_min: i32) -> Option<i32> {
    match &e.rule {
        Rule::Fixed { month, day } => Some(days_from_civil(year, *month, *day)),
        Rule::Computed { base, offset } => {
            let b = match base.as_str() {
                "easter_western" => easter_western(year),
                "easter_orthodox" => easter_orthodox(year),
                _ => return None,
            };
            Some(b + offset)
        }
        Rule::NthWeekday { month, weekday, n } => nth_weekday(year, *month, *weekday, *n),
        Rule::Astro { which } => {
            let w = match (which.as_str(), southern) {
                ("march_equinox", _) => 0,
                ("june_solstice", _) | ("longest_night", true) => 1,
                ("september_equinox", _) => 2,
                ("december_solstice", _) | ("longest_night", false) => 3,
                _ => return None,
            };
            Some(astro_local_day(year, w, offset_min))
        }
        Rule::Weather { .. } | Rule::Weekly { .. } => None,
    }
}

/// Is `e` in its window at `w`; how strong.
pub fn active_one(e: &EventDef, w: &When) -> Option<Active> {
    let (year, _, _) = crate::calendar::civil_from_days(w.day);
    match &e.rule {
        Rule::Weather { trigger } => {
            let on = match trigger.as_str() {
                "first_snowfall_of_season" => w.weather.first_snow,
                "first_warm_day" => w.weather.first_warm,
                "thunderstorm" => w.weather.storm,
                _ => false,
            };
            on.then(|| Active {
                id: e.id.clone(),
                intensity: 1.0,
                days_to_peak: 0,
                anchor_year: year,
            })
        }
        Rule::Weekly {
            weekday: wd,
            from,
            to,
        } => {
            let (f, t) = (hhmm(from)?, hhmm(to)?);
            (weekday(w.day) == *wd && (f..=t).contains(&w.minute)).then(|| Active {
                id: e.id.clone(),
                intensity: 1.0,
                days_to_peak: 0,
                anchor_year: year,
            })
        }
        _ => {
            // The window may cross a year (New Year): look at the neighbours too.
            for y in [year - 1, year, year + 1] {
                let a = anchor(e, y, w.southern, w.offset_min)?;
                let (s, p, en) = (a + e.window.start, a + e.window.peak, a + e.window.end);
                if (s..=en).contains(&w.day) {
                    // Linear from 0.3 at the edges to 1 at the peak.
                    let k = if w.day <= p {
                        if p == s {
                            1.0
                        } else {
                            (w.day - s) as f32 / (p - s) as f32
                        }
                    } else if en == p {
                        1.0
                    } else {
                        1.0 - (w.day - p) as f32 / (en - p) as f32
                    };
                    return Some(Active {
                        id: e.id.clone(),
                        intensity: 0.3 + 0.7 * k,
                        days_to_peak: p - w.day,
                        anchor_year: crate::calendar::civil_from_days(s).0,
                    });
                }
            }
            None
        }
    }
}

/// All events in their windows at `w`, strongest first.
pub fn active(cfg: &EventsConfig, w: &When) -> Vec<Active> {
    let mut v: Vec<Active> = cfg.events.iter().filter_map(|e| active_one(e, w)).collect();
    v.sort_by(|a, b| b.intensity.total_cmp(&a.intensity));
    v
}

/// A keyword: words (norm()), the last may end with `*` — «starts with».
#[derive(Debug, Clone)]
struct Phrase(Vec<(String, bool)>);

impl Phrase {
    fn of(k: &str) -> Option<Self> {
        let words: Vec<(String, bool)> = k
            .split_whitespace()
            .filter_map(|w| {
                let prefix = w.ends_with('*');
                norm(w.trim_end_matches('*')).map(|n| (n, prefix))
            })
            .collect();
        (!words.is_empty()).then_some(Self(words))
    }

    fn matches(&self, tokens: &[&str]) -> bool {
        tokens.windows(self.0.len()).any(|win| {
            win.iter().zip(&self.0).all(|(t, (w, prefix))| {
                if *prefix {
                    t.starts_with(w.as_str())
                } else {
                    *t == w
                }
            })
        })
    }
}

/// How a track suits one event (§11.2): `direct` — its words or genre, `tone` — its vibe.
#[derive(Debug, Clone)]
pub struct Matcher {
    phrases: Vec<Phrase>,
    genre: Vec<(String, f32)>,
    moods: Vec<usize>,
    exclude: Vec<usize>,
    minor: bool,
    valence: Option<[f32; 2]>,
}

impl Matcher {
    pub fn new(e: &EventDef) -> Self {
        let idx = |m: &String| MOODS.iter().position(|x| x == m).map(|i| MOOD0 + i);
        Self {
            phrases: e.keywords.iter().filter_map(|k| Phrase::of(k)).collect(),
            genre: e
                .genre_bonus
                .iter()
                .map(|(g, w)| (g.to_lowercase(), *w))
                .collect(),
            moods: e.tone.moods.iter().filter_map(idx).collect(),
            exclude: e.tone.exclude_moods.iter().filter_map(idx).collect(),
            minor: e.tone.minor,
            valence: e.tone.valence,
        }
    }

    fn words_in(&self, text: Option<&str>) -> bool {
        let Some(n) = text.and_then(norm) else {
            return false;
        };
        let tokens: Vec<&str> = n
            .split(|c: char| !(c.is_alphanumeric() || c == '\''))
            .filter(|t| !t.is_empty())
            .collect();
        self.phrases.iter().any(|p| p.matches(&tokens))
    }

    /// Words in the title 1.0, the album 0.8, the artist 0.4; a genre bonus.
    pub fn direct(&self, t: &WaveTrack) -> f32 {
        let mut s: f32 = 0.0;
        if self.words_in(t.title.as_deref()) {
            s = 1.0;
        } else if self.words_in(t.album.as_deref()) {
            s = 0.8;
        } else if self.words_in(t.artist_name.as_deref()) {
            s = 0.4;
        }
        if let Some(g) = t.genre.as_deref().map(str::to_lowercase) {
            for (k, w) in &self.genre {
                if g.contains(k.as_str()) {
                    s = s.max(*w);
                }
            }
        }
        s
    }

    /// The vibe's membership in the tone, 0…1 (unanalysed — 0). No moods — every analysed
    /// track fits a little (День музыки: «лучшее из медиатеки»).
    pub fn tone(&self, x: &[f32; D], conf: f32) -> f32 {
        if conf <= 0.0 {
            return 0.0;
        }
        if self.exclude.iter().any(|&i| x[i] >= 0.5) {
            return 0.0;
        }
        let mut m = if self.moods.is_empty() {
            0.6
        } else {
            self.moods.iter().map(|&i| x[i]).fold(0.0, f32::max)
        };
        if self.minor {
            m = 0.5 * m + 0.5 * (1.0 - x[MODE]);
        }
        if let Some([lo, hi]) = self.valence {
            let v = x[VALENCE];
            let off = if v < lo {
                lo - v
            } else if v > hi {
                v - hi
            } else {
                0.0
            };
            m *= (1.0 - off * 3.0).max(0.0);
        }
        m.clamp(0.0, 1.0)
    }

    /// `event(t) = max(direct, 0.5·tone)` (§11.2).
    pub fn score(&self, t: &WaveTrack) -> f32 {
        let tone = t.features.map_or(0.0, |(x, c)| self.tone(&x, c));
        self.direct(t).max(0.5 * tone)
    }
}

/// The library's fit for an event: direct matches and tracks that fit its tone (≥ 0.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fit {
    pub direct: usize,
    pub tone: usize,
}

impl Fit {
    /// < 5 direct — the wave plays by tone with the matches spread (§11.2).
    pub fn atmospheric(&self) -> bool {
        self.direct < 5
    }

    /// < 30 by tone (and no direct matches to make up for it) — not offered.
    pub fn offerable(&self) -> bool {
        self.tone + self.direct >= 30
    }
}

pub fn fit(m: &Matcher, tracks: &[WaveTrack]) -> Fit {
    let mut f = Fit::default();
    for t in tracks.iter().filter(|t| !t.never) {
        if m.direct(t) >= 0.5 {
            f.direct += 1;
        } else if t.features.is_some_and(|(x, c)| m.tone(&x, c) >= 0.5) {
            f.tone += 1;
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{civil_from_days, maslenitsa};

    fn d(y: i32, m: u32, dd: u32) -> i32 {
        days_from_civil(y, m, dd)
    }

    fn when(day: i32) -> When {
        When {
            day,
            minute: 12 * 60,
            offset_min: 180,
            southern: false,
            weather: WeatherFlags::default(),
        }
    }

    fn ids(cfg: &EventsConfig, day: i32) -> Vec<String> {
        active(cfg, &when(day)).into_iter().map(|a| a.id).collect()
    }

    #[test]
    fn shipped_events_parse_with_the_q2_defaults() {
        let c = EventsConfig::default();
        for id in [
            "new_year",
            "christmas_orthodox",
            "valentines",
            "defender_day",
            "maslenitsa",
            "womens_day",
            "april_fools",
            "easter_orthodox",
            "victory_day",
            "music_day",
            "knowledge_day",
            "halloween",
        ] {
            assert!(c.get(id).is_some_and(|e| e.default_on), "{id}");
        }
        assert!(!c.get("christmas_western").is_some_and(|e| e.default_on));
        assert!(!c.get("easter_western").is_some_and(|e| e.default_on));
        assert!(!c.get("friday_evening").is_some_and(|e| e.default_on));
        assert!(!c.get("victory_day").is_some_and(|e| e.auto));
    }

    #[test]
    fn windows_of_the_supplement_table() {
        let c = EventsConfig::default();
        // New Year: 15 December – 8 January, peak 31 December — across the year.
        assert!(!ids(&c, d(2026, 12, 14)).contains(&"new_year".into()));
        assert!(ids(&c, d(2026, 12, 15)).contains(&"new_year".into()));
        assert!(ids(&c, d(2027, 1, 8)).contains(&"new_year".into()));
        assert!(!ids(&c, d(2027, 1, 9)).contains(&"new_year".into()));
        let peak = active_one(c.get("new_year").expect("ny"), &when(d(2026, 12, 31)));
        assert_eq!(
            peak.as_ref().map(|a| (a.intensity, a.days_to_peak)),
            Some((1.0, 0))
        );
        // Halloween: 24–31 October; AT-13 runs on the 26th.
        assert!(!ids(&c, d(2026, 10, 23)).contains(&"halloween".into()));
        let h = active_one(c.get("halloween").expect("h"), &when(d(2026, 10, 26))).expect("in");
        assert_eq!(h.days_to_peak, 5);
        assert!(h.intensity > 0.3 && h.intensity < 1.0);
        assert!(!ids(&c, d(2026, 11, 1)).contains(&"halloween".into()));
        // Knowledge day from 29 August.
        assert!(ids(&c, d(2026, 8, 29)).contains(&"knowledge_day".into()));
    }

    #[test]
    fn computed_weekly_weather_and_astro_rules() {
        let c = EventsConfig::default();
        // Maslenitsa = the whole week ending with Forgiveness Sunday.
        let (s, e) = maslenitsa(2027);
        assert!(ids(&c, s).contains(&"maslenitsa".into()));
        assert!(ids(&c, e).contains(&"maslenitsa".into()));
        assert!(!ids(&c, e + 1).contains(&"maslenitsa".into()));
        // Easter both, from the algorithms.
        assert!(ids(&c, easter_orthodox(2028)).contains(&"easter_orthodox".into()));
        assert!(ids(&c, easter_western(2027)).contains(&"easter_western".into()));
        // Friday evening 18:00–23:59 (16.10.2026 is a Friday).
        let fri = d(2026, 10, 16);
        let mut w = when(fri);
        w.minute = 19 * 60;
        assert!(active(&c, &w).iter().any(|a| a.id == "friday_evening"));
        w.minute = 12 * 60;
        assert!(!active(&c, &w).iter().any(|a| a.id == "friday_evening"));
        // Weather: only with the flag.
        let mut w = when(d(2026, 11, 20));
        assert!(!active(&c, &w).iter().any(|a| a.id == "first_snow"));
        w.weather.first_snow = true;
        assert!(active(&c, &w).iter().any(|a| a.id == "first_snow"));
        // The longest night: 21/22 December in the north, June in the south.
        let dec = astro_local_day(2027, 3, 180);
        assert!(ids(&c, dec).contains(&"longest_night".into()));
        let mut south = when(astro_local_day(2027, 1, 600));
        south.southern = true;
        south.offset_min = 600;
        assert!(active(&c, &south).iter().any(|a| a.id == "longest_night"));
    }

    /// AT-24: four years at a 6 h step — every event shows only inside its window, both Easters
    /// and Maslenitsa where the tables put them.
    #[test]
    fn four_years_with_an_accelerated_clock() {
        let c = EventsConfig::default();
        let start = d(2027, 1, 1);
        let end = d(2031, 1, 1);
        let mut seen = std::collections::BTreeMap::<String, usize>::new();
        let mut day = start;
        while day < end {
            for q in 0..4u16 {
                let mut w = when(day);
                w.minute = q * 360;
                for a in active(&c, &w) {
                    let e = c.get(&a.id).expect("known");
                    if let Rule::Fixed { month, day: dd } = e.rule {
                        let (y, _, _) = civil_from_days(day);
                        let ok = [y - 1, y, y + 1].iter().any(|&yy| {
                            let anchor = days_from_civil(yy, month, dd);
                            (anchor + e.window.start..=anchor + e.window.end).contains(&day)
                        });
                        assert!(ok, "{} on {:?}", a.id, civil_from_days(day));
                    }
                    *seen.entry(a.id).or_default() += 1;
                }
            }
            day += 1;
        }
        for (y, m, dd) in [(2027, 5, 2), (2028, 4, 16), (2029, 4, 8), (2030, 4, 28)] {
            assert!(
                ids(&c, d(y, m, dd)).contains(&"easter_orthodox".into()),
                "{y}"
            );
        }
        for (y, m, dd) in [(2027, 3, 28), (2028, 4, 16), (2029, 4, 1), (2030, 4, 21)] {
            assert!(
                ids(&c, d(y, m, dd)).contains(&"easter_western".into()),
                "{y}"
            );
        }
        for (y, m, dd) in [(2027, 3, 8), (2028, 2, 21), (2029, 2, 12), (2030, 3, 4)] {
            assert!(ids(&c, d(y, m, dd)).contains(&"maslenitsa".into()), "{y}");
        }
        // Each dated event happened every one of the four years.
        for id in [
            "new_year",
            "halloween",
            "victory_day",
            "music_day",
            "longest_night",
        ] {
            assert!(seen.get(id).copied().unwrap_or(0) >= 4, "{id}: {seen:?}");
        }
    }

    fn track(title: &str, album: &str, artist: &str, genre: Option<&str>) -> WaveTrack {
        let mut t = crate::synth::library(1, 0, 1.0, 1).remove(0);
        t.title = Some(title.into());
        t.album = Some(album.into());
        t.artist_name = Some(artist.into());
        t.genre = genre.map(Into::into);
        t
    }

    #[test]
    fn words_and_genres_score_directly() {
        let c = EventsConfig::default();
        let ny = Matcher::new(c.get("new_year").expect("ny"));
        assert_eq!(ny.direct(&track("Новогодняя песня", "", "", None)), 1.0);
        assert_eq!(ny.direct(&track("Last Christmas", "", "", None)), 1.0);
        assert_eq!(ny.direct(&track("Песня", "Под ёлкой", "", None)), 0.8);
        assert_eq!(
            ny.direct(&track("Песня", "Альбом", "Santa Band", None)),
            0.4
        );
        assert!(ny.direct(&track("Песня", "Альбом", "Группа", Some("Christmas Pop"))) >= 0.8);
        assert_eq!(
            ny.direct(&track("Летний дождь", "Лето", "Кто-то", Some("Rock"))),
            0.0
        );
    }

    #[test]
    fn the_tone_reads_the_vibe() {
        let c = EventsConfig::default();
        let h = Matcher::new(c.get("halloween").expect("h"));
        let mut gloomy = [0.5f32; D];
        gloomy[MOOD0 + 6] = 0.9;
        gloomy[MODE] = 0.0;
        let mut bright = [0.5f32; D];
        bright[MOOD0] = 0.9;
        bright[MODE] = 1.0;
        assert!(h.tone(&gloomy, 1.0) > 0.8);
        assert!(h.tone(&bright, 1.0) < 0.3);
        assert_eq!(h.tone(&gloomy, 0.0), 0.0, "unanalysed — no tone");
        // 9 May: nothing cheerful even if calm.
        let v = Matcher::new(c.get("victory_day").expect("v"));
        let mut calm_happy = [0.5f32; D];
        calm_happy[MOOD0 + 2] = 0.9;
        calm_happy[MOOD0] = 0.8;
        assert_eq!(v.tone(&calm_happy, 1.0), 0.0);
    }
}
