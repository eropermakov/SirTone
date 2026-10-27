//! The wave engine (docs/14 §1, §4–§9; ported from the S-17 prototype): a catalog of feature
//! vectors and signals in memory → stage 1 scores every track (no allocation in the loop) and
//! keeps the top K per familiarity group → the sequencer picks the next tracks by the slider
//! quotas, Thompson sampling, artist spacing, the repeat window, «никогда» and «не сейчас».
//! Weights and thresholds come from `assets/wave` (profile, engine, genres); time only from the
//! caller (`now_ms`, the `Clock` in the app).

use serde::Deserialize;
use sirtone_core::text::norm;
use sirtone_core::wave::{ContextSnapshot, FEATURES as D, NotNowRow, WaveTrack};

use crate::context::bucket_matches;
use crate::learn::{History, Models, ctx_levels, replay};
use crate::query::{Character, Mood, QueryState, SeasonPick, Vibe, WaveQuery, script_lang};
use crate::rng::Rng;
use crate::season::{NO_SEASON, Seasonality, dominant};
use sirtone_core::wave::{Fit, fit};

/// Feature indices (docs/14 §4.1).
pub mod f {
    pub const ENERGY: usize = 0;
    pub const VALENCE: usize = 1;
    pub const AROUSAL: usize = 2;
    pub const DANCE: usize = 3;
    pub const ACOUSTIC: usize = 8;
    pub const VOCAL: usize = 9;
    pub const BRIGHTNESS: usize = 10;
    pub const AGGRESSION: usize = 15;
    pub const MOOD: usize = 18;
    pub const TEMPO: usize = 4;
    pub const KEY_SIN: usize = 5;
    pub const KEY_COS: usize = 6;
    pub const MODE: usize = 7;
    pub const DYNAMICS: usize = 11;
    pub const LOUDNESS: usize = 12;
    pub const DURATION: usize = 13;
    pub const SOFT_IN: usize = 16;
    pub const SOFT_OUT: usize = 17;
    pub const YEAR: usize = 14;
    pub const GENRE: usize = 26;
    pub const GENRES: usize = 6;
}

const DAY_MS: i64 = 24 * 3600 * 1000;

/// §4.1 №4: (clamp(bpm, 50, 200) − 50) / 150.
fn tempo_feature(bpm: f32) -> f32 {
    ((bpm.clamp(50.0, 200.0) - 50.0) / 150.0).clamp(0.0, 1.0)
}

/// The stored vector of a deeply analysed track (§4.1, `track_features.vec`): the B3 features
/// and the tempo; key, B0 and metadata features are filled from columns when the catalog is
/// built, so they stay current without re-analysis.
pub fn deep_features(v: &sirtone_core::analysis::Vibe, bpm: Option<f32>) -> [f32; D] {
    let mut x = [0.5f32; D];
    x[f::ENERGY] = v.energy;
    x[f::VALENCE] = v.valence;
    x[f::AROUSAL] = v.arousal;
    x[f::DANCE] = v.danceability;
    x[f::ACOUSTIC] = v.acousticness;
    x[f::VOCAL] = v.vocal;
    x[f::BRIGHTNESS] = v.brightness;
    x[f::AGGRESSION] = v.aggression;
    x[f::MOOD..f::MOOD + 8].copy_from_slice(&v.moods);
    x[f::GENRE..f::GENRE + f::GENRES].copy_from_slice(&v.genres);
    if let Some(b) = bpm {
        x[f::TEMPO] = tempo_feature(b);
    }
    x
}

// ---------- configuration (assets/wave) ----------

#[derive(Debug, Clone, Deserialize)]
pub struct Weights {
    pub taste: f32,
    pub session: f32,
    pub context: f32,
    pub season: f32,
    pub event: f32,
    pub curve: f32,
    pub novelty: f32,
    pub universal: f32,
    pub filters: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Sequencing {
    pub artist_spacing: usize,
    pub repeat_window_h: u32,
    pub max_energy_jump: f32,
    pub transition_bonus: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub id: String,
    pub weights: Weights,
    pub sequencing: Sequencing,
    /// The seasonal wave (§10): tracks «убрать из сезонной» and those clearly of another
    /// season (season score below this) are left out.
    #[serde(default)]
    pub season_floor: Option<f32>,
    /// What «Настроить волну» relaxes first when too few tracks fit (§6, `05` §10.1).
    #[serde(default = "default_relax")]
    pub relax_order: Vec<String>,
    /// «Для всех» (§16): the universality score leads and its exclusions hold.
    #[serde(default)]
    pub company: bool,
    /// `full` — the owner's models; `guests` — the separate guests' model only (§8, AT-20).
    #[serde(default = "default_policy")]
    pub learning_policy: String,
    /// «Засыпательная волна» (§14): the energy curve and its filters.
    #[serde(default)]
    pub sleep: Option<SleepRules>,
    /// The focus wave (§15): work or break rules.
    #[serde(default)]
    pub focus: Option<FocusRules>,
    /// The event wave (§11.2).
    #[serde(default)]
    pub event_rules: Option<EventRules>,
}

/// The event wave's rules (`profiles/event.json`).
#[derive(Debug, Clone, Deserialize)]
pub struct EventRules {
    /// A track «fits» the event from this `event(t)`.
    pub fit_min: f32,
    /// With this many fitting tracks the rest stay out of the queue.
    pub fit_enough: usize,
    /// Fewer direct matches — the wave is «атмосферная».
    pub atmospheric_below: usize,
    /// …and a direct match comes at most once in this many tracks.
    pub spread: u32,
}

/// The running event for the event wave: the matcher and what the library has for it.
struct EventRun {
    matcher: crate::events::Matcher,
    /// `event(t)` and «direct match» per catalog index.
    score: Vec<f32>,
    direct: Vec<bool>,
    fit: crate::events::Fit,
    /// Tracks with `event(t) ≥ fit_min`.
    fitting: usize,
}

/// The focus wave's rules (`profiles/focus*.json`, §15, FR-FOCUS-003/005).
#[derive(Debug, Clone, Deserialize)]
pub struct FocusRules {
    pub segment: String,
    #[serde(default)]
    pub energy_target: Option<f32>,
    #[serde(default)]
    pub energy_tol: Option<f32>,
    #[serde(default)]
    pub dynamics_max: Option<f32>,
    /// «Только без слов»: vocals above this are out.
    #[serde(default)]
    pub vocal_hard_max: Option<f32>,
    /// «Предпочтительно без слов»: score − penalty × vocal.
    #[serde(default)]
    pub vocal_penalty: f32,
    pub novelty_max: f32,
    #[serde(default)]
    pub valence_min: Option<f32>,
    #[serde(default)]
    pub energy_min: Option<f32>,
}

/// «Слова» in the focus settings (Q-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Vocals {
    None,
    #[default]
    PreferNone,
    Any,
}

/// The sleep wave's rules (`profiles/sleep.json`, §14, FR-SLEEP-003).
#[derive(Debug, Clone, Deserialize)]
pub struct SleepRules {
    pub e_min: f32,
    pub p: f32,
    pub aggression_max: f32,
    pub energy_margin: f32,
    pub last_third_from: f32,
    pub tempo_max_bpm: f32,
    pub vocal_penalty: f32,
    pub soft_bonus: f32,
    pub novelty_max: f32,
    /// E₀ when the timer does not run (the wave started on its own).
    pub e0_default: f32,
}

impl SleepRules {
    /// `E(t) = E_min + (E₀ − E_min)·(1 − (t/T)^p)`, faster in the second half.
    pub fn energy_at(&self, e0: f32, progress: f32) -> f32 {
        let t = progress.clamp(0.0, 1.0);
        self.e_min + (e0.max(self.e_min) - self.e_min) * (1.0 - t.powf(self.p))
    }
}

/// Where the sleep timer is (the app passes it before each refill): E₀ — the energy of the
/// track playing when it started; `progress` — 0…1 of the timer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SleepState {
    pub e0: f32,
    pub progress: f32,
}

fn default_policy() -> String {
    "full".into()
}

/// «Для всех» rules (`assets/wave/universal.json`, §16).
#[derive(Debug, Clone, Deserialize)]
pub struct UniversalWeights {
    pub popularity: f32,
    pub sound: f32,
    pub genre: f32,
    pub guests: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UniversalSound {
    pub tempo: [f32; 2],
    pub tempo_soft: f32,
    pub duration_s: [f32; 2],
    pub duration_soft_s: f32,
    pub energy_soft: f32,
    pub vocal_min: f32,
    pub dynamics_max: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenreWord {
    pub word: String,
    pub score: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UniversalExclude {
    pub moods: Vec<String>,
    pub characters: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Universal {
    pub weights: UniversalWeights,
    pub popularity_ref_listeners: f32,
    pub sound: UniversalSound,
    pub genre_groups: [f32; 6],
    pub genre_words: Vec<GenreWord>,
    pub hit_bonus: f32,
    pub exclude: UniversalExclude,
    /// Fewer suitable tracks → the honest caption (FR-ALL-005).
    pub few: usize,
}

fn default_repeat_relax_share() -> f32 {
    0.5
}

fn default_relax() -> Vec<String> {
    [
        "filters.atmosphere",
        "filters.activity",
        "filters.character",
        "filters.mood",
        "tempo",
        "energy",
        "language",
        "frequency",
    ]
    .map(String::from)
    .to_vec()
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuotaStop {
    pub at: f32,
    pub groups: [f32; 3],
}

#[derive(Debug, Clone, Deserialize)]
pub struct Familiarity {
    pub loved_plays: u32,
    pub loved_mean: f32,
    pub forgotten_plays: u32,
    pub forgotten_days: u32,
    pub new_days: u32,
    pub new_max_plays: u32,
    pub never_return_mean: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Signals {
    pub like: f32,
    pub never: f32,
    pub skip_early: f32,
    pub skip_mid: f32,
    pub skip_late: f32,
    pub manual: f32,
    pub not_now: f32,
    pub finished: f32,
    pub finished_new: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionCfg {
    pub half_life_min: f32,
    pub window_min: f32,
    pub skip_step: f32,
    pub like_step: f32,
    /// Session skips push away tracks of the same vibe (D-92).
    pub repel: f32,
    pub repel_sigma: f32,
    pub repel_last: usize,
}

/// The vibe features the session repel compares (§4.1): energy, valence, arousal,
/// danceability and the eight mood tags.
const REPEL_DIMS: [usize; 12] = [0, 1, 2, 3, 18, 19, 20, 21, 22, 23, 24, 25];

#[derive(Debug, Clone, Deserialize)]
pub struct EngineParams {
    pub quotas: Vec<QuotaStop>,
    pub top_k: usize,
    pub top_sample: usize,
    pub temperature: f32,
    pub explore_base: f32,
    pub explore_slope: f32,
    pub taste_sigma: f32,
    pub novelty_pull: f32,
    pub cold_conf_floor: f32,
    pub familiarity: Familiarity,
    pub repeat_soft_days: u32,
    /// The repeat rule is breakable (D-119, MusicMaster's rule hierarchy): when the window and
    /// the session leave nothing, this share of the eligible tracks — the least recently heard
    /// or queued — comes back.
    #[serde(default = "default_repeat_relax_share")]
    pub repeat_relax_share: f32,
    pub signals: Signals,
    pub beta_half_life_days: f32,
    /// λ = n/(n + this) for the context levels (§8.2).
    pub context_prior_n: f32,
    /// Fewer eligible tracks under «Настроить волну» → relaxation (§6).
    pub min_candidates: usize,
    pub session: SessionCfg,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Genres {
    pub groups: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub profile: Profile,
    pub engine: EngineParams,
    pub genres: Genres,
    pub seasons: Seasonality,
    pub vibe: Vibe,
    pub universal: Universal,
    /// Profiles whose plays and signals teach only the guests' model (`learning_policy`).
    pub guest_profiles: Vec<String>,
    /// Profiles that teach only their own context (`context_only`, the sleep wave §14): a
    /// finished track is no signal, a skip teaches only the «sleep» context.
    pub context_only_profiles: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
#[error("wave config {file}: {source}")]
pub struct ConfigError {
    file: &'static str,
    source: Box<dyn std::error::Error + Send + Sync>,
}

/// Built-in profiles (docs/14 §5): id → file.
const PROFILES: [(&str, &str, &str); 7] = [
    (
        "my_wave",
        "profiles/my_wave.json",
        include_str!("../../../assets/wave/profiles/my_wave.json"),
    ),
    (
        "seasonal",
        "profiles/seasonal.json",
        include_str!("../../../assets/wave/profiles/seasonal.json"),
    ),
    (
        "for_all",
        "profiles/for_all.json",
        include_str!("../../../assets/wave/profiles/for_all.json"),
    ),
    (
        "sleep",
        "profiles/sleep.json",
        include_str!("../../../assets/wave/profiles/sleep.json"),
    ),
    (
        "focus",
        "profiles/focus.json",
        include_str!("../../../assets/wave/profiles/focus.json"),
    ),
    (
        "focus_break",
        "profiles/focus_break.json",
        include_str!("../../../assets/wave/profiles/focus_break.json"),
    ),
    (
        "event",
        "profiles/event.json",
        include_str!("../../../assets/wave/profiles/event.json"),
    ),
];

impl Config {
    /// The built-in «Моя волна» with the bundled assets.
    pub fn builtin() -> Result<Self, ConfigError> {
        Self::builtin_profile("my_wave")
    }

    /// A built-in profile by id (`my_wave`, `seasonal`, `for_all`); an unknown id is «Моя
    /// волна».
    pub fn builtin_profile(id: &str) -> Result<Self, ConfigError> {
        let (_, file, text) = PROFILES.iter().find(|p| p.0 == id).unwrap_or(&PROFILES[0]);
        Self::with_profile(file, text)
    }

    /// A profile from its JSON (`14` §5.1, schema `sirtone.wave-profile/1`) with the bundled
    /// assets; an invalid one is a clear error naming the file and what is wrong (T3.25).
    pub fn with_profile(file: &'static str, text: &str) -> Result<Self, ConfigError> {
        let parse = |file: &'static str, text: &str| -> Result<serde_json::Value, ConfigError> {
            serde_json::from_str(text).map_err(|e| ConfigError {
                file,
                source: Box::new(e),
            })
        };
        let p = parse(file, text)?;
        let invalid = |what: String| ConfigError {
            file,
            source: what.into(),
        };
        if p["schema"].as_str() != Some("sirtone.wave-profile/1") {
            return Err(invalid("schema must be sirtone.wave-profile/1".into()));
        }
        let e = parse(
            "engine.json",
            include_str!("../../../assets/wave/engine.json"),
        )?;
        let g = parse(
            "genres.json",
            include_str!("../../../assets/wave/genres.json"),
        )?;
        let err = |file: &'static str| {
            move |e: serde_json::Error| ConfigError {
                file,
                source: Box::new(e),
            }
        };
        let u = parse(
            "universal.json",
            include_str!("../../../assets/wave/universal.json"),
        )?;
        let mut guest_profiles = Vec::new();
        let mut context_only_profiles = Vec::new();
        for (pid, pfile, ptext) in PROFILES {
            match parse(pfile, ptext)?["learning_policy"].as_str() {
                Some("guests") => guest_profiles.push(pid.to_string()),
                Some("context_only") => context_only_profiles.push(pid.to_string()),
                _ => {}
            }
        }
        let profile: Profile = serde_json::from_value(p).map_err(err(file))?;
        let w = &profile.weights;
        let weights = [
            ("taste", w.taste),
            ("session", w.session),
            ("context", w.context),
            ("season", w.season),
            ("event", w.event),
            ("curve", w.curve),
            ("novelty", w.novelty),
            ("universal", w.universal),
            ("filters", w.filters),
        ];
        if let Some((name, v)) = weights.iter().find(|(_, v)| !v.is_finite() || *v < 0.0) {
            return Err(invalid(format!("weight {name} = {v}: must be ≥ 0")));
        }
        if weights.iter().map(|(_, v)| v).sum::<f32>() <= 0.0 {
            return Err(invalid("all weights are 0".into()));
        }
        if !["full", "guests", "context_only"].contains(&profile.learning_policy.as_str()) {
            return Err(invalid(format!(
                "learning_policy {}: full, guests or context_only",
                profile.learning_policy
            )));
        }
        if profile.sequencing.artist_spacing == 0 {
            return Err(invalid("sequencing.artist_spacing must be ≥ 1".into()));
        }
        if profile
            .season_floor
            .is_some_and(|f| !(0.0..=1.0).contains(&f))
        {
            return Err(invalid("season_floor must be within 0…1".into()));
        }
        Ok(Self {
            universal: serde_json::from_value(u).map_err(err("universal.json"))?,
            guest_profiles,
            context_only_profiles,
            profile,
            engine: serde_json::from_value(e).map_err(err("engine.json"))?,
            genres: serde_json::from_value(g).map_err(err("genres.json"))?,
            seasons: Seasonality::builtin().map_err(|e| ConfigError {
                file: "seasons.json",
                source: Box::new(e),
            })?,
            vibe: Vibe::builtin().map_err(|e| ConfigError {
                file: "vibe",
                source: Box::new(e),
            })?,
        })
    }

    /// Slider position → group shares, linear between the owner's stops (docs/14 §9).
    pub fn quotas(&self, novelty: f32) -> [f32; 3] {
        let v = novelty.clamp(0.0, 1.0);
        let q = &self.engine.quotas;
        for w in q.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if v <= b.at {
                let t = if b.at > a.at {
                    (v - a.at) / (b.at - a.at)
                } else {
                    0.0
                };
                return std::array::from_fn(|i| a.groups[i] + t * (b.groups[i] - a.groups[i]));
            }
        }
        q.last().map_or([1.0, 0.0, 0.0], |s| s.groups)
    }
}

// ---------- catalog ----------

/// The engine's view of the library: struct of arrays, one index per track.
pub struct Catalog {
    pub id: Vec<i64>,
    pub feat: Vec<[f32; D]>,
    /// Multiplier for tracks without deep analysis (`floor + (1 − floor)·conf`).
    cold: Vec<f32>,
    pub artist: Vec<i64>,
    /// Track length (packing into a focus/sleep segment, §7.1).
    pub(crate) dur_ms: Vec<u64>,
    pub(crate) alpha: Vec<f32>,
    pub(crate) beta: Vec<f32>,
    pub(crate) plays: Vec<u32>,
    /// Analysis confidence (0 — not analysed).
    pub(crate) conf: Vec<f32>,
    /// Language by script: 0 unknown, 1 Russian, 2 foreign.
    pub(crate) lang: Vec<u8>,
    pub(crate) weeks: Vec<[u8; 13]>,
    /// The tempo was measured (B1) — the tempo filter reads this, not the analysis confidence.
    pub(crate) has_bpm: Vec<bool>,
    /// `0.6·ln(1 + plays) + 0.4·completion − 0.5·skip_ratio` (`05` §10.1 «любимые»).
    pub(crate) fav: Vec<f32>,
    pub last_played: Vec<i64>,
    added: Vec<i64>,
    pub(crate) liked: Vec<bool>,
    pub never: Vec<bool>,
    /// Season affinity [winter, spring, summer, autumn] (§10.2), 0.5 = says nothing.
    pub season: Vec<[f32; 4]>,
    /// «Убрать из сезонной» — out of the seasonal wave only.
    pub no_season: Vec<bool>,
    /// The genre tag, lower case (the expert level's exclusions).
    pub(crate) genre: Vec<String>,
    /// «Для всех» (§16): the static part of universality — Σ wᵢ·sᵢ and Σ wᵢ over the parts
    /// the track has (popularity, sound, genre); the guests' part is added per rebuild.
    pub(crate) univ: Vec<f32>,
    pub(crate) univ_w: Vec<f32>,
    /// Passes the «Для всех» exclusions (moods, «не для компании», explicit, analysed).
    pub(crate) company_ok: Vec<bool>,
    /// «Хит для всех».
    pub(crate) hit: Vec<bool>,
    /// The guests' model: beta per track (§8, `separate_model`).
    pub(crate) guest_alpha: Vec<f32>,
    pub(crate) guest_beta: Vec<f32>,
    /// Tracks with known popularity («подборка без данных о популярности» when 0).
    pop_known: usize,
    center: [f32; D],
    index: std::collections::HashMap<i64, u32>,
}

/// Genre tag → group (first hit on the normalised words).
fn genre_group(genre: &str, groups: &[Vec<String>]) -> Option<usize> {
    let g = norm(genre)?;
    groups
        .iter()
        .position(|words| words.iter().any(|w| g.contains(w.as_str())))
}

impl Catalog {
    pub fn build(tracks: &[WaveTrack], cfg: &Config) -> Self {
        let n = tracks.len();
        let e = &cfg.engine;
        // Means of the analysed features (or 0.5) stand in until a track is analysed (§4.1).
        let mut mean = [0.5f32; D];
        let analysed: Vec<&[f32; D]> = tracks
            .iter()
            .filter_map(|t| t.features.as_ref().map(|f| &f.0))
            .collect();
        if !analysed.is_empty() {
            mean = [0.0; D];
            for x in &analysed {
                for (m, v) in mean.iter_mut().zip(x.iter()) {
                    *m += v;
                }
            }
            for v in &mut mean {
                *v /= analysed.len() as f32;
            }
        }
        // Genre shares of the library — the stand-in for an unknown genre.
        let mut shares = [0f32; f::GENRES];
        let mut known = 0f32;
        let groups: Vec<Option<usize>> = tracks
            .iter()
            .map(|t| {
                t.genre
                    .as_deref()
                    .and_then(|g| genre_group(g, &cfg.genres.groups))
            })
            .collect();
        for g in groups.iter().flatten() {
            shares[*g] += 1.0;
            known += 1.0;
        }
        for v in &mut shares {
            *v = if known > 0.0 {
                *v / known
            } else {
                1.0 / f::GENRES as f32
            };
        }
        let mut years: Vec<f32> = tracks
            .iter()
            .filter_map(|t| t.year)
            .map(year_feature)
            .collect();
        years.sort_by(f32::total_cmp);
        let year_mid = years.get(years.len() / 2).copied().unwrap_or(0.5);

        let mut c = Catalog {
            id: Vec::with_capacity(n),
            feat: Vec::with_capacity(n),
            cold: Vec::with_capacity(n),
            artist: Vec::with_capacity(n),
            dur_ms: Vec::with_capacity(n),
            alpha: Vec::with_capacity(n),
            beta: Vec::with_capacity(n),
            plays: Vec::with_capacity(n),
            conf: Vec::with_capacity(n),
            lang: Vec::with_capacity(n),
            weeks: Vec::with_capacity(n),
            has_bpm: Vec::with_capacity(n),
            fav: Vec::with_capacity(n),
            last_played: Vec::with_capacity(n),
            added: Vec::with_capacity(n),
            liked: Vec::with_capacity(n),
            never: Vec::with_capacity(n),
            season: Vec::with_capacity(n),
            no_season: Vec::with_capacity(n),
            genre: Vec::with_capacity(n),
            univ: Vec::with_capacity(n),
            univ_w: Vec::with_capacity(n),
            company_ok: Vec::with_capacity(n),
            hit: Vec::with_capacity(n),
            guest_alpha: vec![1.0; n],
            guest_beta: vec![1.0; n],
            pop_known: 0,
            center: [0.0; D],
            index: std::collections::HashMap::with_capacity(n),
        };
        for (t, g) in tracks.iter().zip(&groups) {
            let (mut x, conf) = t.features.unwrap_or((mean, 0.0));
            // Metadata features are known without analysis (§4.1: «теги»).
            x[f::DURATION] = duration_feature(t.duration_ms);
            // B0 (T3.6): dynamics, loudness, soft intro/outro (§4.1 features 11, 12, 16, 17).
            if let Some(v) = t.lra {
                x[f::DYNAMICS] = (v / 20.0).clamp(0.0, 1.0);
            }
            if let Some(v) = t.lufs {
                x[f::LOUDNESS] = ((v + 30.0) / 25.0).clamp(0.0, 1.0);
            }
            if let Some(v) = t.soft_intro {
                x[f::SOFT_IN] = v;
            }
            if let Some(v) = t.soft_outro {
                x[f::SOFT_OUT] = v;
            }
            // B1 (T3.7): tempo 4 = (clamp(bpm, 50, 200) − 50) / 150; key 5–6 on the circle of
            // fifths (the Camelot number); mode 7 (major 1 / minor 0).
            if let Some(b) = t.bpm {
                x[f::TEMPO] = tempo_feature(b);
            }
            if let Some((n, major)) = t.key_camelot.as_deref().and_then(camelot_parts) {
                let a = (n - 1) as f32 * std::f32::consts::TAU / 12.0;
                x[f::KEY_SIN] = 0.5 + 0.5 * a.sin();
                x[f::KEY_COS] = 0.5 + 0.5 * a.cos();
                x[f::MODE] = f32::from(u8::from(major));
            }
            x[f::YEAR] = t.year.map_or(year_mid, year_feature);
            // Genre: tags first, then the model (B3), then the library's shares (§4.1).
            match g {
                Some(g) => {
                    for k in 0..f::GENRES {
                        x[f::GENRE + k] = f32::from(u8::from(k == *g));
                    }
                }
                None if t.features.is_none() => {
                    x[f::GENRE..f::GENRE + f::GENRES].copy_from_slice(&shares);
                }
                None => {}
            }
            c.index.insert(t.id, c.id.len() as u32);
            c.id.push(t.id);
            c.feat.push(x);
            c.cold
                .push(e.cold_conf_floor + (1.0 - e.cold_conf_floor) * conf);
            c.artist.push(t.artist_id);
            c.dur_ms.push(t.duration_ms);
            // Filled by the replay of the journal (learn.rs).
            c.alpha.push(1.0);
            c.beta.push(1.0);
            c.plays.push(t.play_count);
            c.conf.push(conf);
            c.lang
                .push(script_lang(t.title.as_deref(), t.artist_name.as_deref()));
            c.weeks.push(t.plays_weeks);
            c.has_bpm.push(t.bpm.is_some());
            let heard = (t.play_count + t.skip_count).max(1) as f32;
            c.fav.push(
                0.6 * (1.0 + t.play_count as f32).ln() + 0.4 * t.completion
                    - 0.5 * t.skip_count as f32 / heard,
            );
            c.last_played.push(t.last_played_ms.unwrap_or(i64::MIN / 2));
            c.added.push(t.added_ms);
            c.liked.push(t.liked);
            c.never.push(t.never);
            c.season.push([0.5; 4]);
            c.no_season.push(t.season_tags & NO_SEASON != 0);
            c.genre
                .push(t.genre.as_deref().unwrap_or("").trim().to_lowercase());
        }
        // Cold-start centre of taste: weight ∝ ln(1 + tracks of the artist) (§8.3).
        let mut per_artist = std::collections::HashMap::<i64, u32>::new();
        for a in &c.artist {
            *per_artist.entry(*a).or_default() += 1;
        }
        let mut wsum = 0f32;
        for (i, x) in c.feat.iter().enumerate() {
            let w = (1.0 + per_artist.get(&c.artist[i]).copied().unwrap_or(1) as f32).ln();
            for (cv, v) in c.center.iter_mut().zip(x.iter()) {
                *cv += w * v;
            }
            wsum += w;
        }
        if wsum > 0.0 {
            for v in &mut c.center {
                *v /= wsum;
            }
        }
        c.universal(tracks, &groups, cfg);
        c
    }

    /// The static part of «Для всех» (§16): popularity, sound and genre universality, and the
    /// exclusions.
    fn universal(&mut self, tracks: &[WaveTrack], groups: &[Option<usize>], cfg: &Config) {
        let u = &cfg.universal;
        let w = &u.weights;
        let snd = &u.sound;
        let moods: Vec<Mood> = u
            .exclude
            .moods
            .iter()
            .filter_map(|m| Mood::from_key(m))
            .collect();
        let chars: Vec<Character> = u
            .exclude
            .characters
            .iter()
            .filter_map(|c| Character::from_key(c))
            .collect();
        let words: Vec<(String, f32)> = u
            .genre_words
            .iter()
            .filter_map(|g| norm(&g.word).map(|n| (n, g.score)))
            .collect();
        // Trapezoid: 1 inside [lo, hi], down to 0 over `soft` outside.
        let trap = |v: f32, lo: f32, hi: f32, soft: f32| {
            let miss = (lo - v).max(v - hi).max(0.0);
            (1.0 - miss / soft.max(1e-6)).max(0.0)
        };
        let ref_log = (1.0 + u.popularity_ref_listeners).log10().max(1e-6);
        let c = self.center;
        for (i, t) in tracks.iter().enumerate() {
            let (mut sum, mut wsum) = (0f32, 0f32);
            if let Some(l) = t.listeners {
                self.pop_known += 1;
                sum += w.popularity * ((1.0 + l as f32).log10() / ref_log).min(1.0);
                wsum += w.popularity;
            }
            let analysed = t.features.is_some();
            let x = &self.feat[i];
            if analysed {
                let parts = [
                    // Positive and danceable at least as the library's middle.
                    trap(x[f::VALENCE], c[f::VALENCE], 1.0, 0.15),
                    trap(x[f::DANCE], c[f::DANCE], 1.0, 0.15),
                    // Energy near the middle.
                    (1.0 - (x[f::ENERGY] - c[f::ENERGY]).abs() / snd.energy_soft).max(0.0),
                    trap(x[f::VOCAL], snd.vocal_min, 1.0, 0.2),
                    t.bpm
                        .map_or(0.5, |b| trap(b, snd.tempo[0], snd.tempo[1], snd.tempo_soft)),
                    trap(
                        t.duration_ms as f32 / 1000.0,
                        snd.duration_s[0],
                        snd.duration_s[1],
                        snd.duration_soft_s,
                    ),
                    trap(x[f::DYNAMICS], 0.0, snd.dynamics_max, 0.15),
                ];
                sum += w.sound * parts.iter().sum::<f32>() / parts.len() as f32;
                wsum += w.sound;
            }
            let by_word = t
                .genre
                .as_deref()
                .and_then(norm)
                .and_then(|g| words.iter().find(|(wd, _)| g.contains(wd.as_str())))
                .map(|(_, s)| *s);
            let genre = by_word
                .or_else(|| groups[i].map(|g| u.genre_groups[g]))
                .or_else(|| {
                    analysed.then(|| {
                        (0..f::GENRES)
                            .map(|k| x[f::GENRE + k] * u.genre_groups[k])
                            .sum::<f32>()
                    })
                });
            if let Some(g) = genre {
                sum += w.genre * g;
                wsum += w.genre;
            }
            self.univ.push(sum);
            self.univ_w.push(wsum);
            let conf = t.features.map_or(0.0, |(_, c)| c);
            let fits = cfg.vibe.analysed_enough(conf) && !cfg.vibe.excluded(x, &moods, &chars);
            self.hit.push(t.company > 0);
            self.company_ok
                .push(t.company >= 0 && !t.explicit && (fits || t.company > 0));
        }
    }

    /// How many tracks pass the «Для всех» exclusions (the honest caption below `few`).
    pub fn company_count(&self) -> usize {
        self.company_ok.iter().filter(|v| **v).count()
    }

    /// Tracks with known popularity (the caption «подборка без данных о популярности»).
    pub fn popularity_known(&self) -> usize {
        self.pop_known
    }

    /// Season affinity of every track from its tags, words (cached by id — matched once per
    /// track), the journal and the sound (§10.2). `tracks` are the ones the catalog was built from.
    fn seasons(
        &mut self,
        tracks: &[WaveTrack],
        events: &[sirtone_core::wave::PlayEvent],
        s: &Seasonality,
        words: &mut std::collections::HashMap<i64, [f32; 4]>,
        now_ms: i64,
    ) {
        let (hist, days) = s.history(events, |id| self.idx(id), self.len(), now_ms);
        for (i, t) in tracks.iter().enumerate() {
            let w = *words.entry(t.id).or_insert_with(|| {
                s.words(
                    t.title.as_deref(),
                    t.album.as_deref(),
                    t.artist_name.as_deref(),
                )
            });
            let feat = t.features.map(|(_, conf)| (&self.feat[i], conf));
            self.season[i] = s.affinity(t.season_tags, &w, hist[i], days, feat);
        }
    }

    pub fn len(&self) -> usize {
        self.id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
    }

    pub(crate) fn center(&self) -> [f32; D] {
        self.center
    }

    pub(crate) fn idx(&self, id: i64) -> Option<usize> {
        self.index.get(&id).map(|&i| i as usize)
    }

    pub(crate) fn mean(&self, t: usize) -> f32 {
        self.alpha[t] / (self.alpha[t] + self.beta[t])
    }

    /// Familiarity group for the slider quotas: 0 — loved/familiar, 1 — forgotten,
    /// 2 — new/untouched (docs/14 §9).
    fn group(&self, t: usize, now_ms: i64, fam: &Familiarity) -> usize {
        let plays = self.plays[t];
        let days = (now_ms - self.last_played[t]) / DAY_MS;
        let age_days = (now_ms - self.added[t]) / DAY_MS;
        if self.liked[t] || (plays >= fam.loved_plays && self.mean(t) > fam.loved_mean) {
            0 // любимое
        } else if plays >= fam.forgotten_plays && days > i64::from(fam.forgotten_days) {
            1 // забытое
        } else if age_days <= i64::from(fam.new_days) && plays < fam.new_max_plays {
            2 // новое
        } else if plays >= 1 {
            0 // знакомое
        } else {
            2 // нетронутое
        }
    }
}

/// «8B» → (8, major); «8A» → (8, minor).
pub(crate) fn camelot_parts(s: &str) -> Option<(u32, bool)> {
    let major = s.ends_with('B');
    let n: u32 = s.trim_end_matches(['A', 'B']).parse().ok()?;
    ((1..=12).contains(&n) && (major || s.ends_with('A'))).then_some((n, major))
}

fn duration_feature(ms: u64) -> f32 {
    let s = (ms as f32 / 1000.0).max(1.0);
    ((s.ln() - 60f32.ln()) / (900f32.ln() - 60f32.ln())).clamp(0.0, 1.0)
}

fn year_feature(y: i32) -> f32 {
    ((y as f32 - 1950.0) / 80.0).clamp(0.0, 1.0)
}

// ---------- taste model (docs/14 §8.2) ----------

#[inline(always)]
fn dot(a: &[f32; D], b: &[f32; D]) -> f32 {
    let mut acc = [0f32; 8];
    for i in (0..D).step_by(8) {
        for j in 0..8 {
            acc[j] += a[i + j] * b[i + j];
        }
    }
    acc.iter().sum()
}

#[inline(always)]
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Logistic regression with a diagonal Gaussian posterior (Chapelle & Li 2011).
#[derive(Debug, Clone)]
pub struct Taste {
    m: [f32; D],
    q: [f32; D],
    b: f32,
}

impl Default for Taste {
    fn default() -> Self {
        Self {
            m: [0.0; D],
            q: [1.0; D],
            b: 0.0,
        }
    }
}

impl Taste {
    pub fn update(&mut self, x: &[f32; D], y: f32, w: f32) {
        let p = sigmoid(dot(&self.m, x) + self.b);
        let g = w * (y - p);
        let h = w * p * (1.0 - p);
        for ((q, m), xi) in self.q.iter_mut().zip(self.m.iter_mut()).zip(x.iter()) {
            *q += h * xi * xi;
            *m += g * xi / *q;
        }
        self.b += 0.05 * g;
    }

    pub fn mean(&self) -> &[f32; D] {
        &self.m
    }

    fn sample(&self, rng: &mut Rng, sigma: f32) -> [f32; D] {
        std::array::from_fn(|i| self.m[i] + sigma * rng.normal() / self.q[i].sqrt())
    }
}

// ---------- stage 1 + sequencer ----------

#[derive(Clone, Copy)]
struct Scored {
    score: f32,
    idx: u32,
}

fn keep_top(v: &mut Vec<Scored>, k: usize) {
    if v.len() > k {
        v.select_nth_unstable_by(k, |a, b| b.score.total_cmp(&a.score));
        v.truncate(k);
    }
    v.sort_unstable_by(|a, b| b.score.total_cmp(&a.score));
}

/// What the sequencer remembers between picks in a session.
#[derive(Default)]
struct Session {
    recent_artists: Vec<i64>,
    prev_energy: Option<f32>,
    prev_tempo: Option<f32>,
    group_counts: [u32; 3],
    slots: u32,
    /// Track ids chosen in this session (survive a catalog reload).
    queued: Vec<i64>,
    /// Picks since the last direct event match (the «атмосферная» spread).
    since_direct: u32,
}

pub struct Engine {
    cfg: Config,
    cat: Catalog,
    taste: Taste,
    context: std::collections::HashMap<String, (Taste, u32)>,
    session_vec: [f32; D],
    /// The last session skips from the journal: catalog index and decay (D-92).
    model_skips: Vec<(usize, f32)>,
    /// Skips reported by the app as they happen (the journal writes later): id and time.
    live_skips: Vec<(i64, i64)>,
    /// What repels in this rebuild: features and weight.
    repel: Vec<([f32; D], f32)>,
    /// The last rebuild, for «Почему этот трек» (§17).
    last: Explained,
    buf: [Vec<Scored>; 3],
    blocked: Vec<bool>,
    session: Session,
    rng: Rng,
    /// Season words per track id, matched once (titles rarely change; a refill reuses them).
    words: std::collections::HashMap<i64, [f32; 4]>,
    /// «Настроить волну» and its evaluation on the catalog (T3.18).
    query: WaveQuery,
    qs: QueryState,
    /// The sleep timer for the sleep wave (§14); `None` — not running.
    sleep: Option<SleepState>,
    /// The event of the event wave (§11); `None` — none.
    event: Option<EventRun>,
    /// «Непохожесть» 0…1 of the expert level (FR-NOV-003); `None` — follows the slider.
    distance: Option<f32>,
    /// «Слова» for the focus wave (Q-5).
    vocals: Vocals,
    /// The mode's hard filters (sleep, focus) apply; off for one rebuild when they leave nothing
    /// (a library without fitting tracks must still play, `05` §10.1 relaxation).
    mode_strict: bool,
    /// The repeat window and «once per session» apply; off for one rebuild when they leave
    /// nothing — a library shorter than the window must still play (D-119).
    repeat_strict: bool,
    /// The last batch needed the repeat rule broken (for `explain` and the log).
    repeat_relaxed: bool,
    /// Recency keys of the eligible tracks while the repeat rule is relaxed (reused).
    recency: Vec<i64>,
    /// «Never twice in a session» counts from this index of `session.queued`: 0, or — while
    /// the repeat rule is relaxed — where the relaxed batch began.
    queued_from: usize,
    /// Packing into a segment (§7.1): its time left when the next pick would start, and the
    /// allowed overrun; each pick takes its length off.
    segment: Option<SegmentPack>,
}

/// A focus (or sleep) segment to pack tracks into (§7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentPack {
    pub left_ms: i64,
    pub overrun_ms: i64,
}

impl Engine {
    /// «Только без слов / Предпочтительно без слов / Неважно» for the focus wave.
    pub fn set_vocals(&mut self, v: Vocals) {
        self.vocals = v;
    }

    /// Pack the next picks into a segment (§7.1); `None` — no boundary.
    pub fn set_segment(&mut self, s: Option<SegmentPack>) {
        self.segment = s;
    }

    /// What is left of the segment after the picks so far.
    pub fn segment(&self) -> Option<SegmentPack> {
        self.segment
    }

    /// The sleep timer's state for the next refill (the sleep wave's energy curve).
    pub fn set_sleep(&mut self, s: Option<SleepState>) {
        self.sleep = s;
    }

    /// Builds the catalog and replays the statistics into the taste model once (§8.3 (2)).
    pub fn new(
        cfg: Config,
        tracks: &[WaveTrack],
        history: &History,
        now_ms: i64,
        seed: u64,
    ) -> Self {
        let mut words = std::collections::HashMap::with_capacity(tracks.len());
        let (cat, models) = Self::learnt(&cfg, tracks, history, &mut words, now_ms);
        let model_skips = models.skips.clone();
        let n = cat.len();
        let query = WaveQuery::default();
        let qs = Self::evaluate(&cfg, &cat, &query, now_ms);
        let cap = n.min(cfg.engine.top_k * 64).max(1);
        Self {
            blocked: vec![false; n],
            buf: [
                Vec::with_capacity(cap),
                Vec::with_capacity(cap),
                Vec::with_capacity(cap),
            ],
            cfg,
            cat,
            taste: models.taste,
            context: models.context,
            session_vec: models.session,
            model_skips,
            live_skips: Vec::new(),
            repel: Vec::new(),
            last: Explained::default(),
            session: Session::default(),
            rng: Rng::new(seed),
            words,
            query,
            qs,
            sleep: None,
            event: None,
            distance: None,
            vocals: Vocals::default(),
            mode_strict: true,
            repeat_strict: true,
            repeat_relaxed: false,
            recency: Vec::with_capacity(n),
            queued_from: 0,
            segment: None,
        }
    }

    fn evaluate(cfg: &Config, cat: &Catalog, q: &WaveQuery, now_ms: i64) -> QueryState {
        QueryState::evaluate(cat, q, cfg, now_ms)
    }

    /// «Подходит N треков» in «Настроить волну» (T5.7, `06` §3): tracks the filters `q` let
    /// through, and whether they had to be relaxed to get enough.
    pub fn query_count(&self, q: &WaveQuery, now_ms: i64) -> (usize, bool) {
        let qs = Self::evaluate(&self.cfg, &self.cat, q, now_ms);
        (
            qs.pass.iter().filter(|p| **p).count(),
            !qs.relaxed.is_empty(),
        )
    }

    /// «Настроить волну» (T3.18): the filters for this session; returns the relaxation steps
    /// taken («Мало треков — расширил подбор» when not empty).
    pub fn set_query(&mut self, q: WaveQuery, now_ms: i64) -> Vec<&'static str> {
        self.query = q;
        self.qs = Self::evaluate(&self.cfg, &self.cat, &self.query, now_ms);
        self.qs.relaxed.clone()
    }

    /// The last batch broke the repeat rule: the library is shorter than the window (D-119).
    pub fn repeat_relaxed(&self) -> bool {
        self.repeat_relaxed
    }

    /// The relaxation steps of the current query.
    pub fn relaxed(&self) -> &[&'static str] {
        &self.qs.relaxed
    }

    /// The profile this engine plays (`my_wave`, `seasonal`, `for_all`).
    pub fn profile_id(&self) -> &str {
        &self.cfg.profile.id
    }

    /// «Для всех» captions (FR-ALL-005): no popularity data at all; fewer suitable tracks than
    /// the honest limit. Both false outside «Для всех».
    pub fn company_captions(&self) -> (bool, bool) {
        if !self.cfg.profile.company {
            return (false, false);
        }
        (
            self.cat.popularity_known() == 0,
            self.cat.company_count() < self.cfg.universal.few,
        )
    }

    /// Fresh statistics and signals (after plays, likes, imports), the session kept: spacing,
    /// what was already queued and the group counts carry on.
    pub fn reload(&mut self, tracks: &[WaveTrack], history: &History, now_ms: i64) {
        let (cat, models) = Self::learnt(&self.cfg, tracks, history, &mut self.words, now_ms);
        self.model_skips = models.skips.clone();
        self.blocked = vec![false; cat.len()];
        self.cat = cat;
        self.taste = models.taste;
        self.context = models.context;
        self.session_vec = models.session;
        self.qs = Self::evaluate(&self.cfg, &self.cat, &self.query, now_ms);
        if let Some(run) = self.event.take() {
            self.event = Some(self.event_run(run.matcher, tracks));
        }
    }

    /// «Неслушанность» and «непохожесть» apart (expert, FR-NOV-003, `14` §9): the slider then
    /// sets only the quotas; `None` — one slider for both.
    pub fn set_distance(&mut self, d: Option<f32>) {
        self.distance = d.map(|v| v.clamp(0.0, 1.0));
    }

    /// The event for the event wave (§11.2): scores every track, counts the fit.
    pub fn set_event(&mut self, def: Option<&crate::events::EventDef>, tracks: &[WaveTrack]) {
        self.event = def.map(|d| self.event_run(crate::events::Matcher::new(d), tracks));
        self.session.since_direct = u32::MAX / 2;
    }

    /// What the library has for the event: direct matches, tracks by tone.
    pub fn event_fit(&self) -> Option<crate::events::Fit> {
        self.event.as_ref().map(|e| e.fit)
    }

    fn event_run(&self, matcher: crate::events::Matcher, tracks: &[WaveTrack]) -> EventRun {
        let n = self.cat.len();
        let mut score = vec![0f32; n];
        let mut direct = vec![false; n];
        for t in tracks {
            if let Some(i) = self.cat.idx(t.id) {
                score[i] = matcher.score(t);
                direct[i] = matcher.direct(t) >= 0.5;
            }
        }
        let fit_min = self
            .cfg
            .profile
            .event_rules
            .as_ref()
            .map_or(0.25, |r| r.fit_min);
        EventRun {
            fit: crate::events::fit(&matcher, tracks),
            fitting: score.iter().filter(|s| **s >= fit_min).count(),
            matcher,
            score,
            direct,
        }
    }

    fn learnt(
        cfg: &Config,
        tracks: &[WaveTrack],
        history: &History,
        words: &mut std::collections::HashMap<i64, [f32; 4]>,
        now_ms: i64,
    ) -> (Catalog, Models) {
        let mut cat = Catalog::build(tracks, cfg);
        cat.seasons(tracks, &history.events, &cfg.seasons, words, now_ms);
        let models = replay(cfg, &cat, history, now_ms);
        cat.alpha.clone_from(&models.alpha);
        cat.beta.clone_from(&models.beta);
        cat.guest_alpha.clone_from(&models.guest_alpha);
        cat.guest_beta.clone_from(&models.guest_beta);
        (cat, models)
    }

    /// A wave track was skipped just now (the app reports it at once; the journal writes the
    /// play later): it repels its vibe from the next picks (D-92, AT-21).
    pub fn skipped(&mut self, id: i64, now_ms: i64) {
        self.live_skips.retain(|(i, _)| *i != id);
        self.live_skips.push((id, now_ms));
        let keep = self.cfg.engine.session.repel_last;
        if self.live_skips.len() > keep {
            self.live_skips.drain(..self.live_skips.len() - keep);
        }
    }

    /// The repel set of this moment: the journal's skips and the live ones (no track twice),
    /// analysed tracks only, the last `repel_last`.
    fn repel_now(&mut self, now_ms: i64) {
        let s = &self.cfg.engine.session;
        let decay = |ts: i64| {
            let age = (now_ms - ts).max(0) as f32 / 60_000.0;
            (age <= s.window_min).then(|| 2f32.powf(-age / s.half_life_min))
        };
        let mut set: Vec<(usize, f32)> = self.model_skips.clone();
        for &(id, ts) in &self.live_skips {
            if let (Some(i), Some(k)) = (self.cat.idx(id), decay(ts))
                && self.cat.conf[i] > 0.0
                && !set.iter().any(|(j, _)| *j == i)
            {
                set.push((i, k));
            }
        }
        if set.len() > s.repel_last {
            set.drain(..set.len() - s.repel_last);
        }
        self.repel.clear();
        self.repel
            .extend(set.into_iter().map(|(i, w)| (self.cat.feat[i], w)));
    }

    pub fn catalog(&self) -> &Catalog {
        &self.cat
    }

    /// The next `n` tracks (ids) for the context. `novelty` — the slider, 0…1.
    pub fn next(
        &mut self,
        ctx: &ContextSnapshot,
        not_now: &[NotNowRow],
        novelty: f32,
        n: usize,
        now_ms: i64,
    ) -> Vec<i64> {
        // A chosen «Частота» is a hard filter and the slider is inactive (Q-12, D-42).
        let novelty = if self.qs.frequency_on { 0.5 } else { novelty };
        // The sleep and focus waves keep novelty low («незнакомое будит / отвлекает»).
        let novelty = match (&self.cfg.profile.sleep, &self.cfg.profile.focus) {
            (Some(r), _) => novelty.min(r.novelty_max),
            (_, Some(r)) => novelty.min(r.novelty_max),
            _ => novelty,
        };
        self.rebuild(ctx, not_now, novelty, now_ms);
        let moded = self.cfg.profile.sleep.is_some() || self.cfg.profile.focus.is_some();
        if moded && self.buf.iter().all(Vec::is_empty) {
            self.mode_strict = false;
            self.rebuild(ctx, not_now, novelty, now_ms);
            self.mode_strict = true;
        }
        // The rule hierarchy when nothing passes (D-119, MusicMaster): first the sequencing
        // (artist spacing, the energy jump) bends while unheard tracks remain; then the repeat
        // rule — the least recently heard or queued come back. «никогда», disliked, «не сейчас»
        // and «Для всех» never bend.
        self.repeat_relaxed = false;
        self.queued_from = 0;
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            let t = match self.pick(novelty, false) {
                Some(t) => Some(t),
                None => self.pick(novelty, true),
            };
            match t {
                Some(t) => out.push(self.cat.id[t as usize]),
                None if !self.repeat_relaxed => {
                    self.repeat_relaxed = true;
                    self.repeat_strict = false;
                    self.rebuild(ctx, not_now, novelty, now_ms);
                    self.repeat_strict = true;
                    self.queued_from = self.session.queued.len();
                }
                None => break,
            }
        }
        out
    }

    /// Familiarity group of a track (0 — loved/familiar, 1 — forgotten, 2 — new/untouched).
    pub fn group_of(&self, id: i64, now_ms: i64) -> Option<usize> {
        let i = self.cat.idx(id)?;
        Some(self.cat.group(i, now_ms, &self.cfg.engine.familiarity))
    }

    /// A track was heard (now in the repeat window).
    pub fn played(&mut self, id: i64, now_ms: i64) {
        if let Some(i) = self.cat.idx(id) {
            self.cat.last_played[i] = now_ms;
        }
    }

    /// Online learning from one signal (§8.1): `y` 1/0 with weight `w`.
    pub fn learn(&mut self, id: i64, y: f32, w: f32) {
        if let Some(i) = self.cat.idx(id) {
            if y > 0.5 {
                self.cat.alpha[i] += w;
            } else {
                self.cat.beta[i] += w;
            }
            let x = self.cat.feat[i];
            self.taste.update(&x, y, w);
        }
    }

    /// «Никогда» now (the mask, without a catalog reload).
    pub fn never(&mut self, id: i64, artist: bool) {
        if let Some(i) = self.cat.idx(id) {
            if artist {
                let a = self.cat.artist[i];
                for (k, v) in self.cat.never.iter_mut().enumerate() {
                    if self.cat.artist[k] == a {
                        *v = true;
                    }
                }
            } else {
                self.cat.never[i] = true;
            }
        }
    }

    /// Stage 1: every track → top K per familiarity group.
    fn rebuild(&mut self, ctx: &ContextSnapshot, not_now: &[NotNowRow], novelty: f32, now_ms: i64) {
        self.repel_now(now_ms);
        let cfg = &self.cfg;
        let e = &cfg.engine;
        let pw = &cfg.profile.weights;
        // The event's weight goes to taste unless an event runs (§5.2) or its chip is off. Season
        // pick and influence of «Настроить волну» rescale the weights (T3.18).
        let w_event = if self.event.is_some() {
            pw.event * self.query.influence[2].clamp(0.0, 2.0)
        } else {
            0.0
        };
        let [w_taste, w_context, w_session, w_season, w_filters, w_univ] = QueryState::weights(
            &self.query,
            [
                pw.taste + pw.event - w_event,
                pw.context,
                pw.session,
                pw.season,
                pw.filters,
                pw.universal,
            ],
        );
        let company = cfg.profile.company;
        let uw_guests = cfg.universal.weights.guests;
        let hit_bonus = cfg.universal.hit_bonus;
        // Atmosphere and season of «Настроить волну» steer scoring; «не сейчас» keeps the
        // real context (D-77).
        let mut sctx = ctx.clone();
        if let Some(d) = self.query.daypart {
            sctx.daypart = d;
        }
        if let Some(wc) = self.query.weather {
            sctx.weather = Some(wc);
        }
        if let SeasonPick::Only(s) = self.query.season {
            sctx.season_w = std::array::from_fn(|i| f32::from(u8::from(i == usize::from(s))));
        }
        let sw = sctx.season_w;
        let season_floor = cfg.profile.season_floor;
        let mut w_ctx = context_weights(&self.context, &sctx, e.context_prior_n);
        // A mode's own context model (`context_only`: the sleep wave, focus — FR-SLEEP-008,
        // FR-FOCUS-008) is what «подходит для сна / фокуса» learnt from skips there.
        if cfg.profile.learning_policy == "context_only"
            && let Some((m, n)) = self.context.get(&cfg.profile.id)
        {
            let lambda = *n as f32 / (*n as f32 + e.context_prior_n);
            for (wi, mi) in w_ctx.iter_mut().zip(m.mean().iter()) {
                *wi += lambda * mi;
            }
        }
        let strict = self.mode_strict;
        let focus = cfg.profile.focus.as_ref().filter(|_| strict);
        let vocals = self.vocals;
        let vibe = &cfg.vibe;
        // The sleep wave (§14): its own context model, the energy curve and its filters.
        let sleep = cfg.profile.sleep.as_ref().map(|r| {
            let st = self.sleep.unwrap_or(SleepState {
                e0: r.e0_default,
                progress: 0.0,
            });
            let last_third = st.progress >= r.last_third_from;
            (
                r,
                r.energy_at(st.e0, st.progress) + r.energy_margin,
                last_third,
                tempo_feature(r.tempo_max_bpm),
            )
        });
        let w_sess = self.session_vec;
        let repel = &self.repel;
        let repel_w = e.session.repel;
        let inv2s2 = 1.0 / (2.0 * e.session.repel_sigma * e.session.repel_sigma).max(1e-6);
        // «Непохожесть» (expert, FR-NOV-003): its own slider, else the main one.
        let distance = self.distance.unwrap_or(novelty);
        let sigma = e.taste_sigma * (0.5 + distance);
        let wt = self.taste.sample(&mut self.rng, sigma);
        let bias = self.taste.b;
        let soft_ms = i64::from(e.repeat_soft_days) * DAY_MS;
        let strict_repeat = self.repeat_strict;
        // «Не сейчас» in this context bucket blocks for its shelf time (D-77).
        self.blocked.fill(false);
        // Already queued in this session — never twice (the top K must not fill with them),
        // unless the repeat rule is relaxed: then queued counts as heard just now, in order.
        // (The buffer is the engine's own: no allocation on the hot path.)
        let mut recent = std::mem::take(&mut self.recency);
        if !strict_repeat {
            recent.clear();
            recent.extend_from_slice(&self.cat.last_played);
        }
        let nq = self.session.queued.len() as i64;
        for (k, &q) in self.session.queued.iter().enumerate() {
            if let Some(i) = self.cat.idx(q) {
                if strict_repeat {
                    self.blocked[i] = true;
                } else {
                    recent[i] = recent[i].max(now_ms - (nq - k as i64));
                }
            }
        }
        // The hard window; relaxed — the least recent share of what the other rules allow.
        let repeat_cut = if strict_repeat {
            now_ms - i64::from(cfg.profile.sequencing.repeat_window_h) * 3600 * 1000
        } else {
            let cat = &self.cat;
            let mut keys: Vec<i64> = (0..cat.len())
                .filter(|&t| {
                    self.qs.pass[t]
                        && !cat.never[t]
                        && !self.blocked[t]
                        && cat.mean(t) >= e.familiarity.never_return_mean
                        && !(!self.query.exclude_genres.is_empty()
                            && self.query.exclude_genres.contains(&cat.genre[t]))
                })
                .map(|t| recent[t])
                .collect();
            if keys.is_empty() {
                i64::MIN
            } else {
                let share = e.repeat_relax_share.clamp(0.0, 1.0);
                let k = ((keys.len() as f32 * share).ceil() as usize).clamp(1, keys.len()) - 1;
                *keys.select_nth_unstable(k).1
            }
        };
        for row in not_now {
            if row.expires_ms > now_ms
                && bucket_matches(&row.ctx_key, ctx)
                && let Some(i) = self.cat.idx(row.track_id)
            {
                self.blocked[i] = true;
            }
        }
        for g in &mut self.buf {
            g.clear();
        }
        let mut sums = [0f32; 5];
        let mut counted = 0u32;
        let cat = &self.cat;
        let fam = &e.familiarity;
        let qs = &self.qs;
        for t in 0..cat.len() {
            // Layer 1: «никогда», disliked, «не сейчас», the hard repeat window; layer 3:
            // «Настроить волну».
            if !qs.pass[t]
                || cat.never[t]
                || (!self.query.exclude_genres.is_empty()
                    && self.query.exclude_genres.contains(&cat.genre[t]))
                || self.blocked[t]
                || cat.mean(t) < fam.never_return_mean
                || (strict_repeat && cat.last_played[t] > repeat_cut)
                || (!strict_repeat && recent.get(t).is_some_and(|&r| r > repeat_cut))
            {
                continue;
            }
            let a = &cat.season[t];
            let season = sw[0] * a[0] + sw[1] * a[1] + sw[2] * a[2] + sw[3] * a[3];
            if let Some(floor) = season_floor
                && (cat.no_season[t] || season < floor)
            {
                continue;
            }
            // «Для всех» (§16): exclusions are hard, never relaxed.
            if company && !cat.company_ok[t] {
                continue;
            }
            let x = &cat.feat[t];
            // The event wave (§11.2): with enough fitting tracks the rest stay out.
            if let (Some(ev), Some(r)) = (&self.event, &cfg.profile.event_rules)
                && ev.fitting >= r.fit_enough
                && ev.score[t] < r.fit_min
            {
                continue;
            }
            // The sleep wave's hard filters: nothing aggressive, nothing above the curve, slow
            // in the last third (only what the analysis knows; unanalysed tracks carry means).
            if let Some((r, e_max, last_third, tempo_max)) = sleep
                && strict
                && cat.conf[t] > 0.0
                && (x[f::AGGRESSION] > r.aggression_max
                    || x[f::ENERGY] > e_max
                    || (last_third && x[f::TEMPO] > tempo_max))
            {
                continue;
            }
            // The focus wave's hard filters (§15), only on what the analysis knows.
            if let Some(r) = focus
                && cat.conf[t] > 0.0
                && (r
                    .energy_target
                    .zip(r.energy_tol)
                    .is_some_and(|(e0, tol)| (x[f::ENERGY] - e0).abs() > tol)
                    || r.dynamics_max.is_some_and(|m| x[f::DYNAMICS] > m)
                    || r.valence_min.is_some_and(|m| x[f::VALENCE] < m)
                    || r.energy_min.is_some_and(|m| x[f::ENERGY] < m)
                    || (r.segment == "work"
                        && vibe.excluded(x, &[Mood::Sad], &[Character::Aggressive]))
                    || (vocals == Vocals::None
                        && r.vocal_hard_max.is_some_and(|m| x[f::VOCAL] > m)))
            {
                continue;
            }
            let taste = 0.6 * sigmoid(dot(&wt, x) + bias) + 0.4 * cat.mean(t);
            let context = sigmoid(dot(&w_ctx, x));
            let session = 0.5 + 0.5 * dot(&w_sess, x).tanh();
            let universal = if w_univ > 0.0 {
                let g = cat.guest_alpha[t] / (cat.guest_alpha[t] + cat.guest_beta[t]);
                let u = (cat.univ[t] + uw_guests * g) / (cat.univ_w[t] + uw_guests);
                u + if cat.hit[t] { hit_bonus } else { 0.0 }
            } else {
                0.0
            };
            let mut s = w_taste * taste
                + w_context * context
                + w_session * session
                + w_season * season
                + w_filters * qs.filters[t]
                + w_univ * universal;
            sums[0] += w_taste * taste;
            sums[1] += w_context * context;
            sums[2] += w_session * session;
            sums[3] += w_season * season;
            sums[4] += w_univ * universal;
            counted += 1;
            let g = cat.group(t, now_ms, fam);
            if g == 2 && distance > 0.0 {
                let mut d2 = 0.0;
                for (i, v) in x.iter().enumerate() {
                    let d = v - cat.center[i];
                    d2 += d * d;
                }
                s += e.novelty_pull * distance * (d2 / D as f32).sqrt();
            }
            // The session's skips push away the same vibe (D-92).
            if !repel.is_empty() && cat.conf[t] > 0.0 {
                let mut p = 0f32;
                for (y, w) in repel {
                    let mut d2 = 0f32;
                    for &k in &REPEL_DIMS {
                        let d = x[k] - y[k];
                        d2 += d * d;
                    }
                    p += w * (-(d2 / REPEL_DIMS.len() as f32) * inv2s2).exp();
                }
                s -= repel_w * p.min(1.0);
            }
            if let Some(r) = focus
                && vocals == Vocals::PreferNone
            {
                s -= r.vocal_penalty * x[f::VOCAL];
            }
            if let Some(ev) = &self.event {
                s += w_event * ev.score[t];
            }
            // The sleep wave: soft intros and outros up; words down in the last third.
            if let Some((r, _, last_third, _)) = sleep {
                s += r.soft_bonus * 0.5 * (x[f::SOFT_IN] + x[f::SOFT_OUT]);
                if last_third {
                    s -= r.vocal_penalty * x[f::VOCAL];
                }
            }
            // Soft repeat penalty for a week after the hard window (§6).
            let since = now_ms - cat.last_played[t];
            if since < soft_ms {
                s -= 0.05 / (1.0 + (since / DAY_MS) as f32);
            }
            self.buf[g].push(Scored {
                score: s * cat.cold[t],
                idx: t as u32,
            });
        }
        for g in &mut self.buf {
            keep_top(g, e.top_k);
        }
        self.recency = recent;
        let k = counted.max(1) as f32;
        self.last = Explained {
            wt,
            bias,
            w_ctx,
            w_sess,
            w: [w_taste, w_context, w_session, w_season, w_univ],
            mean: sums.map(|v| v / k),
            season_w: sw,
            daypart: sctx.daypart as u8,
            now_ms,
        };
    }

    /// «Почему этот трек» (§17, FR-LEARN-009): the 1–2 largest contributions over the mean of
    /// the last rebuild's candidates, plus the familiarity of the track (new / forgotten).
    pub fn why(&self, id: i64) -> Vec<Why> {
        let Some(t) = self.cat.idx(id) else {
            return Vec::new();
        };
        let l = &self.last;
        let x = &self.cat.feat[t];
        let taste = 0.6 * sigmoid(dot(&l.wt, x) + l.bias) + 0.4 * self.cat.mean(t);
        let context = sigmoid(dot(&l.w_ctx, x));
        let session = 0.5 + 0.5 * dot(&l.w_sess, x).tanh();
        let a = &self.cat.season[t];
        let season: f32 = (0..4).map(|s| l.season_w[s] * a[s]).sum();
        let universal = if l.w[4] > 0.0 {
            let c = &self.cat;
            let g = c.guest_alpha[t] / (c.guest_alpha[t] + c.guest_beta[t]);
            let gw = self.cfg.universal.weights.guests;
            (c.univ[t] + gw * g) / (c.univ_w[t] + gw)
                + if c.hit[t] {
                    self.cfg.universal.hit_bonus
                } else {
                    0.0
                }
        } else {
            0.0
        };
        let mut parts = [
            (l.w[0] * taste - l.mean[0], 0usize),
            (l.w[1] * context - l.mean[1], 1),
            (l.w[2] * session - l.mean[2], 2),
            (l.w[3] * season - l.mean[3], 3),
            (l.w[4] * universal - l.mean[4], 4),
        ];
        parts.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut out = Vec::with_capacity(3);
        match self.cat.group(t, l.now_ms, &self.cfg.engine.familiarity) {
            2 => out.push(Why::NewForYou),
            1 => out.push(Why::Forgotten),
            _ => {}
        }
        // «Подходит к сезону» only for a track that leans to the season (not a neutral 0.5).
        let leans = a[dominant(&l.season_w)] > 0.55;
        for (delta, c) in parts {
            if out.len() >= 2 || delta <= 0.0 {
                break;
            }
            if c == 3 && !leans {
                continue;
            }
            out.push(match c {
                0 if self.cat.liked[t] => Why::Liked,
                0 => {
                    let g = x[f::GENRE..f::GENRE + f::GENRES]
                        .iter()
                        .position(|&v| v >= 0.99);
                    g.map_or(Why::Taste, |g| Why::Genre(g as u8))
                }
                1 => Why::Context(l.daypart),
                2 => Why::Session,
                4 => Why::ForAll,
                _ => Why::Season(dominant(&l.season_w) as u8),
            });
        }
        if out.is_empty() {
            out.push(Why::Taste);
        }
        out
    }

    /// Stage 2: the group furthest behind its quota → Thompson + constraints → softmax.
    /// `loose` — the sequencing constraints (artist spacing, energy jump) bend (D-119).
    fn pick(&mut self, novelty: f32, loose: bool) -> Option<u32> {
        let e = &self.cfg.engine;
        let seq = &self.cfg.profile.sequencing;
        let q = self.cfg.quotas(novelty);
        let ss = &mut self.session;
        let deficit = |g: usize| q[g] * (ss.slots + 1) as f32 - ss.group_counts[g] as f32;
        let mut order = [0usize, 1, 2];
        order.sort_by(|&a, &b| deficit(b).total_cmp(&deficit(a)));
        let explore = e.explore_base + e.explore_slope * self.distance.unwrap_or(novelty);
        let spacing = self
            .query
            .artist_spacing
            .unwrap_or(seq.artist_spacing)
            .max(1);
        let cat = &self.cat;
        // «Частота» on: no quotas — one pool over all groups (Q-12).
        let merged = self.qs.frequency_on;
        const ALL: [usize; 3] = [0, 1, 2];
        let mut pool: Vec<(f32, u32, usize)> = Vec::with_capacity(e.top_sample * 3);
        // Groups with no quota are a last resort (a small library must still play).
        for pass in 0..2 {
            for (oi, &g) in order.iter().enumerate() {
                if merged && (pass > 0 || oi > 0) {
                    break;
                }
                if !merged && pass == 0 && q[g] <= 0.0 {
                    continue;
                }
                pool.clear();
                let groups: &[usize] = if merged {
                    &ALL
                } else {
                    std::slice::from_ref(&order[oi])
                };
                for &pg in groups {
                    let mut taken = 0;
                    for c in &self.buf[pg] {
                        let t = c.idx as usize;
                        // «Атмосферная» event wave: a direct match at most once in `spread`.
                        let spaced_out = self.event.as_ref().is_some_and(|ev| {
                            ev.fit.atmospheric()
                                && ev.direct[t]
                                && ss.since_direct
                                    < self
                                        .cfg
                                        .profile
                                        .event_rules
                                        .as_ref()
                                        .map_or(5, |r| r.spread)
                        });
                        if ss.queued[self.queued_from.min(ss.queued.len())..].contains(&cat.id[t])
                            || (!loose && ss.recent_artists.contains(&cat.artist[t]))
                            || spaced_out
                        {
                            continue;
                        }
                        let x = &cat.feat[t];
                        if !loose
                            && let Some(pe) = ss.prev_energy
                            && (x[f::ENERGY] - pe).abs() > seq.max_energy_jump
                        {
                            continue;
                        }
                        let (a, b) = (cat.alpha[t], cat.beta[t]);
                        let mut s = c.score + explore * (self.rng.beta(a, b) - a / (a + b));
                        if seq.transition_bonus
                            && let Some(pt) = ss.prev_tempo
                            && pt > 0.0
                            && ((x[f::TEMPO] - pt) / pt).abs() <= 0.08
                        {
                            s += 0.02;
                        }
                        pool.push((s, c.idx, pg));
                        taken += 1;
                        if taken == e.top_sample {
                            break;
                        }
                    }
                }
                if pool.is_empty() {
                    continue;
                }
                // A segment boundary (§7.1): only tracks that leave room for another or end
                // at the boundary; by score, and among near-equal ones (±0.02) the one ending
                // closest to the boundary. Nothing fits → the shortest (the caller fades over).
                let packed = self.segment.map(|sg| {
                    let fits: Vec<(f32, u32, usize, Fit)> = pool
                        .iter()
                        .map(|&(sc, idx, pg)| {
                            let d = cat.dur_ms[idx as usize] as i64;
                            (sc, idx, pg, fit(sg.left_ms, d, sg.overrun_ms))
                        })
                        .filter(|p| p.3 != Fit::No)
                        .collect();
                    if fits.is_empty() {
                        let &(_, idx, pg) = pool
                            .iter()
                            .min_by_key(|p| cat.dur_ms[p.1 as usize])
                            .unwrap_or(&pool[0]);
                        return (idx, pg);
                    }
                    let best = fits.iter().map(|p| p.0).fold(f32::MIN, f32::max);
                    let near = |p: &&(f32, u32, usize, Fit)| p.0 >= best - 0.02;
                    let boundary = fits
                        .iter()
                        .filter(near)
                        .filter(|p| p.3 == Fit::Boundary)
                        .min_by_key(|p| (sg.left_ms - cat.dur_ms[p.1 as usize] as i64).abs());
                    let top = fits.iter().max_by(|a, b| a.0.total_cmp(&b.0));
                    let &(_, idx, pg, _) = boundary.or(top).unwrap_or(&fits[0]);
                    (idx, pg)
                });
                if let Some((chosen, cg)) = packed {
                    let t = chosen as usize;
                    if let Some(sg) = self.segment.as_mut() {
                        sg.left_ms -= cat.dur_ms[t] as i64;
                    }
                    ss.queued.push(cat.id[t]);
                    ss.recent_artists.push(cat.artist[t]);
                    if ss.recent_artists.len() > spacing.saturating_sub(1) {
                        ss.recent_artists.remove(0);
                    }
                    ss.prev_energy = Some(cat.feat[t][f::ENERGY]);
                    ss.prev_tempo = Some(cat.feat[t][f::TEMPO]);
                    ss.group_counts[cg] += 1;
                    ss.slots += 1;
                    ss.since_direct = if self.event.as_ref().is_some_and(|e| e.direct[t]) {
                        0
                    } else {
                        ss.since_direct.saturating_add(1)
                    };
                    return Some(chosen);
                }
                let max = pool.iter().map(|p| p.0).fold(f32::MIN, f32::max);
                let total: f32 = pool
                    .iter()
                    .map(|p| ((p.0 - max) / e.temperature).exp())
                    .sum();
                let mut r = self.rng.f32() * total;
                let (mut chosen, mut cg) = (pool[pool.len() - 1].1, pool[pool.len() - 1].2);
                for &(s, idx, pg) in &pool {
                    r -= ((s - max) / e.temperature).exp();
                    if r <= 0.0 {
                        (chosen, cg) = (idx, pg);
                        break;
                    }
                }
                let t = chosen as usize;
                ss.queued.push(cat.id[t]);
                ss.recent_artists.push(cat.artist[t]);
                if ss.recent_artists.len() > spacing.saturating_sub(1) {
                    ss.recent_artists.remove(0);
                }
                ss.prev_energy = Some(cat.feat[t][f::ENERGY]);
                ss.prev_tempo = Some(cat.feat[t][f::TEMPO]);
                ss.group_counts[cg] += 1;
                ss.slots += 1;
                ss.since_direct = if self.event.as_ref().is_some_and(|e| e.direct[t]) {
                    0
                } else {
                    ss.since_direct.saturating_add(1)
                };
                return Some(chosen);
            }
        }
        None
    }
}

/// Effective context weights for a moment: Σ λₗ·mₗ over its levels, λ = n/(n + prior) (§8.2).
fn context_weights(
    models: &std::collections::HashMap<String, (Taste, u32)>,
    ctx: &ContextSnapshot,
    prior_n: f32,
) -> [f32; D] {
    let mut w = [0f32; D];
    for key in ctx_levels(ctx) {
        if let Some((m, n)) = models.get(&key) {
            let lambda = *n as f32 / (*n as f32 + prior_n);
            for (wi, mi) in w.iter_mut().zip(m.mean().iter()) {
                *wi += lambda * mi;
            }
        }
    }
    w
}

/// A reason of «Почему этот трек» (§17); the UI words it (`wave.why.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    Liked,
    /// The genre group (0…5, `assets/wave/genres.json`) the taste likes.
    Genre(u8),
    Taste,
    /// Often heard at this daypart.
    Context(u8),
    Session,
    NewForYou,
    Forgotten,
    /// Fits the season (0 winter … 3 autumn).
    Season(u8),
    /// «Для всех»: liked by many, easy to listen to together.
    ForAll,
}

#[derive(Default)]
struct Explained {
    wt: [f32; D],
    bias: f32,
    w_ctx: [f32; D],
    w_sess: [f32; D],
    w: [f32; 5],
    mean: [f32; 5],
    season_w: [f32; 4],
    daypart: u8,
    now_ms: i64,
}
