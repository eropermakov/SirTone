//! `WaveQuery` — «Настроить волну» as layer 3 of the engine (T3.18, docs/05 §10.1, docs/14 §1).
//!
//! Hard filters: language, frequency, energy range, tempo range (workout tempo). Soft filters:
//! mood, character, activity — membership functions (docs/04 §10.2, `assets/vibe`); their
//! product is the `filters` component, and a track is eligible only at product ≥ 0.5 (all the
//! chosen filters hold, AT-08). Within a block the chips are «or», blocks combine with «and».
//! Atmosphere (daypart, weather) and season override the scoring context; influence scales the
//! season/context weights. Fewer than `min_candidates` eligible → relaxation along the profile's
//! `relax_order`: a soft filter widens (×2), then drops; user hard filters drop last; «никогда»
//! and the hard repeat window are never relaxed.
//!
//! Mood, character, activity, energy and tempo need deep analysis (B1–B3): a track with
//! `conf < conf_min` is «не определено» and passes none of them — library means never stand in.
//! Everything is computed once per query and catalog; the hot path reads a bool and an f32.

use serde::Deserialize;
use sirtone_core::wave::{Daypart, FEATURES as D, WeatherClass};

use crate::engine::{Catalog, Config};

const DAY_MS: i64 = 24 * 3600 * 1000;
const WEEK_MS: i64 = 7 * DAY_MS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Happy,
    Upbeat,
    Calm,
    Sad,
    Positive,
    Negative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Character {
    Energetic,
    Aggressive,
    Calm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Ru,
    Foreign,
    /// Without words (needs the vocal feature, B2/B3).
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Loved,
    Often,
    Forgotten,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activity {
    Workout,
    Sleep,
    Work,
    Road,
    Rest,
    Party,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SeasonPick {
    #[default]
    Auto,
    /// 0 winter … 3 autumn.
    Only(u8),
    Off,
}

macro_rules! keys {
    ($t:ty { $($k:literal => $v:expr),* $(,)? }) => {
        impl $t {
            pub fn from_key(s: &str) -> Option<Self> {
                Some(match s { $($k => $v,)* _ => return None })
            }
        }
    };
}
keys!(Mood { "happy" => Mood::Happy, "upbeat" => Mood::Upbeat, "calm" => Mood::Calm,
    "sad" => Mood::Sad, "positive" => Mood::Positive, "negative" => Mood::Negative });
keys!(Character { "energetic" => Character::Energetic, "aggressive" => Character::Aggressive,
    "calm" => Character::Calm });
keys!(Lang { "ru" => Lang::Ru, "foreign" => Lang::Foreign, "none" => Lang::None });
keys!(Freq { "loved" => Freq::Loved, "often" => Freq::Often, "forgotten" => Freq::Forgotten });
keys!(Activity { "workout" => Activity::Workout, "sleep" => Activity::Sleep, "work" => Activity::Work,
    "road" => Activity::Road, "rest" => Activity::Rest, "party" => Activity::Party });

/// «Настроить волну». Empty = no filter (the default wave).
#[derive(Debug, Clone)]
pub struct WaveQuery {
    pub mood: Vec<Mood>,
    pub character: Vec<Character>,
    pub language: Vec<Lang>,
    pub frequency: Vec<Freq>,
    pub activity: Vec<Activity>,
    /// `light` / `medium` / `intense` (`activities.json`), a hard tempo range.
    pub workout_tempo: Option<String>,
    pub daypart: Option<Daypart>,
    pub weather: Option<WeatherClass>,
    pub season: SeasonPick,
    /// Influence of season, context and events, 0…2 (1 = as the profile says).
    pub influence: [f32; 3],
    /// Energy 0…1.
    pub energy: Option<(f32, f32)>,
    /// Tempo in BPM.
    pub tempo: Option<(f32, f32)>,
}

impl Default for WaveQuery {
    fn default() -> Self {
        Self {
            mood: Vec::new(),
            character: Vec::new(),
            language: Vec::new(),
            frequency: Vec::new(),
            activity: Vec::new(),
            workout_tempo: None,
            daypart: None,
            weather: None,
            season: SeasonPick::Auto,
            influence: [1.0; 3],
            energy: None,
            tempo: None,
        }
    }
}

/// What can be relaxed (names as in `relax_order`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crit {
    Mood,
    Character,
    Activity,
    Atmosphere,
    Tempo,
    Energy,
    Language,
    Frequency,
}

impl Crit {
    pub const ALL: [Crit; 8] = [
        Crit::Mood,
        Crit::Character,
        Crit::Activity,
        Crit::Atmosphere,
        Crit::Tempo,
        Crit::Energy,
        Crit::Language,
        Crit::Frequency,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Crit::Mood => "mood",
            Crit::Character => "character",
            Crit::Activity => "activity",
            Crit::Atmosphere => "atmosphere",
            Crit::Tempo => "tempo",
            Crit::Energy => "energy",
            Crit::Language => "language",
            Crit::Frequency => "frequency",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        let s = s.strip_prefix("filters.").unwrap_or(s);
        Self::ALL.into_iter().find(|c| c.key() == s)
    }

    /// Soft filters widen once, then drop; hard ones only drop.
    fn max_level(self) -> u8 {
        match self {
            Crit::Mood | Crit::Character | Crit::Activity => 2,
            _ => 1,
        }
    }
}

// ---------- vibe config (assets/vibe) ----------

#[derive(Debug, Clone, Copy)]
struct Range {
    f: usize,
    min: f32,
    max: f32,
}

/// Feature index; `bpm` → the tempo feature read as BPM.
const BPM: usize = usize::MAX;

fn feature(name: &str) -> Option<usize> {
    Some(match name {
        "energy" => 0,
        "valence" => 1,
        "arousal" => 2,
        "danceability" => 3,
        "acousticness" => 8,
        "vocal" => 9,
        "aggression" => 15,
        "bpm" => BPM,
        _ => return None,
    })
}

#[derive(Debug, Clone, Deserialize)]
struct RangeJson {
    f: String,
    min: Option<f32>,
    max: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
struct MoodsJson {
    schema: String,
    conf_min: f32,
    sigma: f32,
    prototypes: std::collections::HashMap<String, [f32; 2]>,
    positive_min: f32,
    negative_max: f32,
    edge: f32,
    character: std::collections::HashMap<String, Vec<RangeJson>>,
    soft: f32,
}

#[derive(Debug, Clone, Deserialize)]
struct ActivitiesJson {
    schema: String,
    soft: f32,
    soft_bpm: f32,
    activities: std::collections::HashMap<String, Vec<RangeJson>>,
    workout_tempo: std::collections::HashMap<String, [f32; 2]>,
}

#[derive(Debug, thiserror::Error)]
pub enum VibeError {
    #[error("vibe config: {0}")]
    Json(#[from] serde_json::Error),
    #[error("vibe config: {0}")]
    Invalid(String),
}

/// Membership functions of the soft filters (docs/04 §10.2).
#[derive(Debug, Clone)]
pub struct Vibe {
    conf_min: f32,
    sigma: f32,
    /// happy, upbeat, calm, sad.
    proto: [[f32; 2]; 4],
    positive_min: f32,
    negative_max: f32,
    edge: f32,
    character: [Vec<Range>; 3],
    soft: f32,
    activity: [Vec<Range>; 6],
    act_soft: f32,
    soft_bpm: f32,
    workout: std::collections::HashMap<String, (f32, f32)>,
}

impl Vibe {
    pub fn builtin() -> Result<Self, VibeError> {
        Self::from_json(
            include_str!("../../../assets/vibe/moods.json"),
            include_str!("../../../assets/vibe/activities.json"),
        )
    }

    pub fn from_json(moods: &str, activities: &str) -> Result<Self, VibeError> {
        let m: MoodsJson = serde_json::from_str(moods)?;
        let a: ActivitiesJson = serde_json::from_str(activities)?;
        if m.schema != "sirtone.moods/1" || a.schema != "sirtone.activities/1" {
            return Err(VibeError::Invalid("schema".into()));
        }
        let ranges = |v: Option<&Vec<RangeJson>>, name: &str| -> Result<Vec<Range>, VibeError> {
            let v = v.ok_or_else(|| VibeError::Invalid(name.into()))?;
            v.iter()
                .map(|r| {
                    let f = feature(&r.f).ok_or_else(|| VibeError::Invalid(r.f.clone()))?;
                    Ok(Range {
                        f,
                        min: r.min.unwrap_or(f32::MIN),
                        max: r.max.unwrap_or(f32::MAX),
                    })
                })
                .collect()
        };
        let proto = |k: &str| {
            m.prototypes
                .get(k)
                .copied()
                .ok_or_else(|| VibeError::Invalid(k.into()))
        };
        Ok(Self {
            conf_min: m.conf_min,
            sigma: m.sigma.max(1e-3),
            proto: [
                proto("happy")?,
                proto("upbeat")?,
                proto("calm")?,
                proto("sad")?,
            ],
            positive_min: m.positive_min,
            negative_max: m.negative_max,
            edge: m.edge.max(1e-3),
            character: [
                ranges(m.character.get("energetic"), "energetic")?,
                ranges(m.character.get("aggressive"), "aggressive")?,
                ranges(m.character.get("calm"), "calm")?,
            ],
            soft: m.soft.max(1e-3),
            activity: [
                ranges(a.activities.get("workout"), "workout")?,
                ranges(a.activities.get("sleep"), "sleep")?,
                ranges(a.activities.get("work"), "work")?,
                ranges(a.activities.get("road"), "road")?,
                ranges(a.activities.get("rest"), "rest")?,
                ranges(a.activities.get("party"), "party")?,
            ],
            act_soft: a.soft.max(1e-3),
            soft_bpm: a.soft_bpm.max(1e-3),
            workout: a
                .workout_tempo
                .into_iter()
                .map(|(k, [lo, hi])| (k, (lo, hi)))
                .collect(),
        })
    }

    fn analysed(&self, conf: f32) -> bool {
        conf >= self.conf_min
    }

    /// Mood membership; `widen` 1 doubles σ and the edges.
    fn mood(&self, x: &[f32; D], m: Mood, widen: f32) -> f32 {
        let (v, a) = (x[1], x[2]);
        let kernel = |p: [f32; 2]| {
            let s = self.sigma * widen;
            let d2 = (v - p[0]).powi(2) + (a - p[1]).powi(2);
            (-d2 / (2.0 * s * s)).exp()
        };
        let e = self.edge * widen;
        match m {
            Mood::Happy => kernel(self.proto[0]),
            Mood::Upbeat => kernel(self.proto[1]),
            Mood::Calm => kernel(self.proto[2]),
            Mood::Sad => kernel(self.proto[3]),
            Mood::Positive => (1.0 - (self.positive_min - v) / e).clamp(0.0, 1.0),
            Mood::Negative => (1.0 - (v - self.negative_max) / e).clamp(0.0, 1.0),
        }
    }

    /// Trapezoid: 1 inside [min, max], linear to 0 over `soft` outside; all ranges must hold.
    fn ranges(&self, x: &[f32; D], rs: &[Range], soft: f32, soft_bpm: f32) -> f32 {
        rs.iter()
            .map(|r| {
                let (v, s) = if r.f == BPM {
                    (bpm(x), soft_bpm)
                } else {
                    (x[r.f], soft)
                };
                let miss = (r.min - v).max(v - r.max).max(0.0);
                (1.0 - miss / s).max(0.0)
            })
            .fold(1.0, f32::min)
    }

    /// The workout tempo range in BPM by name.
    pub fn workout_tempo(&self, name: &str) -> Option<(f32, f32)> {
        self.workout.get(name).copied()
    }
}

/// Tempo feature (§4.1: (clamp(bpm, 50, 200) − 50) / 150) back to BPM.
fn bpm(x: &[f32; D]) -> f32 {
    50.0 + 150.0 * x[4]
}

// ---------- language by script (docs/04 §10.2: «письменность артиста/названия») ----------

/// 0 — unknown, 1 — Russian (Cyrillic dominates), 2 — foreign (Latin and other scripts).
pub fn script_lang(title: Option<&str>, artist: Option<&str>) -> u8 {
    let (mut cyr, mut other) = (0u32, 0u32);
    for c in title.into_iter().chain(artist).flat_map(str::chars) {
        if matches!(c, '\u{0400}'..='\u{04FF}') {
            cyr += 1;
        } else if c.is_alphabetic() {
            other += 1;
        }
    }
    if cyr == 0 && other == 0 {
        0
    } else if cyr >= other {
        1
    } else {
        2
    }
}

// ---------- evaluation ----------

/// The query evaluated on a catalog: eligibility and the `filters` component per track, the
/// relaxation steps taken, and whether «Частота» is on (the slider quotas are off then, Q-12).
#[derive(Debug, Clone, Default)]
pub struct QueryState {
    pub pass: Vec<bool>,
    pub filters: Vec<f32>,
    pub relaxed: Vec<&'static str>,
    pub frequency_on: bool,
    /// Tempo range in BPM after the workout tempo is folded in.
    tempo: Option<(f32, f32)>,
}

/// Plays in the last 30 days from the weekly ring (bucket `i` = week `last − i`).
pub(crate) fn plays_30d(weeks: &[u8; 13], last_ms: i64, now_ms: i64) -> u32 {
    let shift = (now_ms.div_euclid(WEEK_MS) - last_ms.div_euclid(WEEK_MS)).max(0);
    weeks
        .iter()
        .enumerate()
        .filter(|(i, _)| (shift + *i as i64) * 7 <= 30)
        .map(|(_, &b)| u32::from(b))
        .sum()
}

struct Levels([u8; 8]);

impl Levels {
    fn get(&self, c: Crit) -> u8 {
        self.0[c as usize]
    }
    fn on(&self, c: Crit) -> bool {
        self.get(c) < c.max_level()
    }
    fn widen(&self, c: Crit) -> f32 {
        if self.get(c) == 1 { 2.0 } else { 1.0 }
    }
}

impl QueryState {
    /// Evaluates `q` on `cat`, relaxing along `relax_order` until `min_candidates` are eligible.
    pub fn evaluate(cat: &Catalog, q: &WaveQuery, cfg: &Config, now_ms: i64) -> Self {
        let vibe = &cfg.vibe;
        let fam = &cfg.engine.familiarity;
        let never_return_mean = fam.never_return_mean;
        let relax_order = &cfg.profile.relax_order;
        let min_candidates = cfg.engine.min_candidates;
        let n = cat.len();
        let tempo = q
            .workout_tempo
            .as_deref()
            .and_then(|w| vibe.workout_tempo(w))
            .or(q.tempo);
        let active = |c: Crit| match c {
            Crit::Mood => !q.mood.is_empty(),
            Crit::Character => !q.character.is_empty(),
            Crit::Activity => !q.activity.is_empty(),
            // Atmosphere steers the context; it never removes tracks.
            Crit::Atmosphere => false,
            Crit::Tempo => tempo.is_some(),
            Crit::Energy => q.energy.is_some(),
            Crit::Language => !q.language.is_empty(),
            Crit::Frequency => !q.frequency.is_empty(),
        };
        let mut st = Self {
            pass: vec![true; n],
            filters: vec![0.5; n],
            relaxed: Vec::new(),
            frequency_on: false,
            tempo,
        };
        if !Crit::ALL.into_iter().any(active) {
            return st;
        }
        // «Любимые» when nothing qualifies: the top 10 % by fav_score (docs/05 §10.1).
        let loved = |t: usize| {
            cat.liked[t] || (cat.plays[t] >= fam.loved_plays && cat.mean(t) > fam.loved_mean)
        };
        let fav_cut = if (0..n).any(loved) {
            None
        } else {
            let mut s: Vec<f32> = (0..n).map(|t| cat.fav[t]).filter(|v| *v > 0.0).collect();
            s.sort_by(|a, b| b.total_cmp(a));
            s.get(s.len() / 10).copied()
        };
        let mut lv = Levels([0; 8]);
        let order: Vec<Crit> = relax_order
            .iter()
            .filter_map(|s| Crit::from_key(s))
            .chain(Crit::ALL)
            .collect();
        loop {
            let mut eligible = 0usize;
            for t in 0..n {
                let x = &cat.feat[t];
                let analysed = vibe.analysed(cat.conf[t]);
                let mut ok = true;
                if lv.on(Crit::Language) && active(Crit::Language) {
                    let lang = cat.lang[t];
                    ok &= q.language.iter().any(|l| match l {
                        Lang::Ru => lang == 1,
                        Lang::Foreign => lang == 2,
                        Lang::None => analysed && x[9] < 0.2,
                    });
                }
                if ok && lv.on(Crit::Frequency) && active(Crit::Frequency) {
                    let since = now_ms - cat.last_played[t];
                    ok &= q.frequency.iter().any(|f| match f {
                        Freq::Loved => {
                            loved(t) || fav_cut.is_some_and(|c| cat.fav[t] >= c && c > 0.0)
                        }
                        Freq::Often => plays_30d(&cat.weeks[t], cat.last_played[t], now_ms) >= 3,
                        Freq::Forgotten => {
                            cat.plays[t] >= fam.forgotten_plays
                                && since > i64::from(fam.forgotten_days) * DAY_MS
                        }
                    });
                }
                if ok
                    && lv.on(Crit::Energy)
                    && let Some((lo, hi)) = q.energy
                {
                    ok &= analysed && (lo..=hi).contains(&x[0]);
                }
                if ok
                    && lv.on(Crit::Tempo)
                    && let Some((lo, hi)) = tempo
                {
                    ok &= analysed && (lo..=hi).contains(&bpm(x));
                }
                let mut f = 1.0f32;
                let mut soft = false;
                if lv.on(Crit::Mood) && active(Crit::Mood) {
                    soft = true;
                    let w = lv.widen(Crit::Mood);
                    f *= if analysed {
                        q.mood
                            .iter()
                            .map(|m| vibe.mood(x, *m, w))
                            .fold(0.0, f32::max)
                    } else {
                        0.0
                    };
                }
                if lv.on(Crit::Character) && active(Crit::Character) {
                    soft = true;
                    let w = lv.widen(Crit::Character);
                    f *= if analysed {
                        q.character
                            .iter()
                            .map(|c| {
                                vibe.ranges(
                                    x,
                                    &vibe.character[*c as usize],
                                    vibe.soft * w,
                                    vibe.soft_bpm * w,
                                )
                            })
                            .fold(0.0, f32::max)
                    } else {
                        0.0
                    };
                }
                if lv.on(Crit::Activity) && active(Crit::Activity) {
                    soft = true;
                    let w = lv.widen(Crit::Activity);
                    f *= if analysed {
                        q.activity
                            .iter()
                            .map(|a| {
                                vibe.ranges(
                                    x,
                                    &vibe.activity[*a as usize],
                                    vibe.act_soft * w,
                                    vibe.soft_bpm * w,
                                )
                            })
                            .fold(0.0, f32::max)
                    } else {
                        0.0
                    };
                }
                let f = if soft { f } else { 0.5 };
                ok &= !soft || f >= 0.5;
                st.pass[t] = ok;
                st.filters[t] = f;
                // Layer 1 (never relaxed) only counts here; the engine applies it anyway.
                if ok && !cat.never[t] && cat.mean(t) >= never_return_mean {
                    eligible += 1;
                }
            }
            if eligible >= min_candidates {
                break;
            }
            let Some(c) = order.iter().copied().find(|c| active(*c) && lv.on(*c)) else {
                break;
            };
            lv.0[c as usize] += 1;
            if !st.relaxed.contains(&c.key()) {
                st.relaxed.push(c.key());
            }
        }
        st.frequency_on = active(Crit::Frequency) && lv.on(Crit::Frequency);
        st
    }

    /// The scoring weights after the query's season pick and influence: season/context scaled
    /// by 0…2, then all rescaled to the original total. Order: taste, context, session, season.
    pub fn weights(q: &WaveQuery, w: [f32; 5]) -> [f32; 5] {
        let [taste, context, session, season, filters] = w;
        let ms = if q.season == SeasonPick::Off {
            0.0
        } else {
            q.influence[0].clamp(0.0, 2.0)
        };
        let mc = q.influence[1].clamp(0.0, 2.0);
        let total = taste + context + session + season + filters;
        let out = [taste, context * mc, session, season * ms, filters];
        let sum: f32 = out.iter().sum();
        if sum <= 1e-6 {
            return w;
        }
        out.map(|v| v * total / sum)
    }

    pub fn tempo(&self) -> Option<(f32, f32)> {
        self.tempo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_decides_the_language() {
        assert_eq!(script_lang(Some("Звезда по имени Солнце"), Some("Кино")), 1);
        assert_eq!(script_lang(Some("Yesterday"), Some("The Beatles")), 2);
        assert_eq!(script_lang(Some("Track 01"), None), 2);
        assert_eq!(script_lang(Some("01"), None), 0);
        // Mixed: the dominant script.
        assert_eq!(script_lang(Some("Love story"), Some("Би-2")), 2);
    }

    #[test]
    fn plays_in_the_last_month() {
        let mut w = [0u8; 13];
        w[0] = 2;
        w[1] = 1;
        w[6] = 9;
        let now = 1_800_000_000_000;
        assert_eq!(plays_30d(&w, now, now), 3);
        // Five weeks later nothing is recent.
        assert_eq!(plays_30d(&w, now, now + 5 * WEEK_MS), 0);
    }

    #[test]
    fn mood_membership_and_widening() {
        let v = Vibe::builtin().unwrap();
        let mut x = [0.5f32; D];
        x[1] = 0.8;
        x[2] = 0.7;
        assert!(v.mood(&x, Mood::Happy, 1.0) > 0.99);
        assert!(v.mood(&x, Mood::Sad, 1.0) < 0.1);
        x[1] = 0.55;
        x[2] = 0.45;
        let narrow = v.mood(&x, Mood::Happy, 1.0);
        let wide = v.mood(&x, Mood::Happy, 2.0);
        assert!(wide > narrow, "{narrow} {wide}");
        x[1] = 0.7;
        assert!(v.mood(&x, Mood::Positive, 1.0) >= 1.0);
        assert!(v.mood(&x, Mood::Negative, 1.0) <= 0.0);
    }

    #[test]
    fn influence_rescales_to_the_same_total() {
        let mut q = WaveQuery::default();
        let w = [0.45, 0.15, 0.1, 0.1, 0.1];
        assert_eq!(QueryState::weights(&q, w), w);
        q.season = SeasonPick::Off;
        let o = QueryState::weights(&q, w);
        assert_eq!(o[3], 0.0);
        assert!((o.iter().sum::<f32>() - 0.9).abs() < 1e-5);
        q.season = SeasonPick::Auto;
        q.influence = [2.0, 0.0, 1.0];
        let o = QueryState::weights(&q, w);
        assert_eq!(o[1], 0.0);
        assert!(o[3] > w[3]);
    }
}
