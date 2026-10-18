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
use crate::query::{QueryState, SeasonPick, Vibe, WaveQuery, script_lang};
use crate::rng::Rng;
use crate::season::{NO_SEASON, Seasonality, dominant};

/// Feature indices (docs/14 §4.1).
pub mod f {
    pub const ENERGY: usize = 0;
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
}

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
}

#[derive(Debug, thiserror::Error)]
#[error("wave config {file}: {source}")]
pub struct ConfigError {
    file: &'static str,
    source: Box<dyn std::error::Error + Send + Sync>,
}

/// Built-in profiles (docs/14 §5): id → file.
const PROFILES: [(&str, &str, &str); 2] = [
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
];

impl Config {
    /// The built-in «Моя волна» with the bundled assets.
    pub fn builtin() -> Result<Self, ConfigError> {
        Self::builtin_profile("my_wave")
    }

    /// A built-in profile by id (`my_wave`, `seasonal`); an unknown id is «Моя волна».
    pub fn builtin_profile(id: &str) -> Result<Self, ConfigError> {
        let parse = |file: &'static str, text: &str| -> Result<serde_json::Value, ConfigError> {
            serde_json::from_str(text).map_err(|e| ConfigError {
                file,
                source: Box::new(e),
            })
        };
        let (_, file, text) = PROFILES.iter().find(|p| p.0 == id).unwrap_or(&PROFILES[0]);
        let p = parse(file, text)?;
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
        Ok(Self {
            profile: serde_json::from_value(p).map_err(err(file))?,
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
                x[f::TEMPO] = ((b.clamp(50.0, 200.0) - 50.0) / 150.0).clamp(0.0, 1.0);
            }
            if let Some((n, major)) = t.key_camelot.as_deref().and_then(camelot_parts) {
                let a = (n - 1) as f32 * std::f32::consts::TAU / 12.0;
                x[f::KEY_SIN] = 0.5 + 0.5 * a.sin();
                x[f::KEY_COS] = 0.5 + 0.5 * a.cos();
                x[f::MODE] = f32::from(u8::from(major));
            }
            x[f::YEAR] = t.year.map_or(year_mid, year_feature);
            if t.features.is_none() {
                match g {
                    Some(g) => {
                        for k in 0..f::GENRES {
                            x[f::GENRE + k] = f32::from(u8::from(k == *g));
                        }
                    }
                    None => x[f::GENRE..f::GENRE + f::GENRES].copy_from_slice(&shares),
                }
            }
            c.index.insert(t.id, c.id.len() as u32);
            c.id.push(t.id);
            c.feat.push(x);
            c.cold
                .push(e.cold_conf_floor + (1.0 - e.cold_conf_floor) * conf);
            c.artist.push(t.artist_id);
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
        c
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
fn camelot_parts(s: &str) -> Option<(u32, bool)> {
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
}

pub struct Engine {
    cfg: Config,
    cat: Catalog,
    taste: Taste,
    context: std::collections::HashMap<String, (Taste, u32)>,
    session_vec: [f32; D],
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
}

impl Engine {
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
            last: Explained::default(),
            session: Session::default(),
            rng: Rng::new(seed),
            words,
            query,
            qs,
        }
    }

    fn evaluate(cfg: &Config, cat: &Catalog, q: &WaveQuery, now_ms: i64) -> QueryState {
        QueryState::evaluate(cat, q, cfg, now_ms)
    }

    /// «Настроить волну» (T3.18): the filters for this session; returns the relaxation steps
    /// taken («Мало треков — расширил подбор» when not empty).
    pub fn set_query(&mut self, q: WaveQuery, now_ms: i64) -> Vec<&'static str> {
        self.query = q;
        self.qs = Self::evaluate(&self.cfg, &self.cat, &self.query, now_ms);
        self.qs.relaxed.clone()
    }

    /// The relaxation steps of the current query.
    pub fn relaxed(&self) -> &[&'static str] {
        &self.qs.relaxed
    }

    /// The profile this engine plays (`my_wave`, `seasonal`).
    pub fn profile_id(&self) -> &str {
        &self.cfg.profile.id
    }

    /// Fresh statistics and signals (after plays, likes, imports), the session kept: spacing,
    /// what was already queued and the group counts carry on.
    pub fn reload(&mut self, tracks: &[WaveTrack], history: &History, now_ms: i64) {
        let (cat, models) = Self::learnt(&self.cfg, tracks, history, &mut self.words, now_ms);
        self.blocked = vec![false; cat.len()];
        self.cat = cat;
        self.taste = models.taste;
        self.context = models.context;
        self.session_vec = models.session;
        self.qs = Self::evaluate(&self.cfg, &self.cat, &self.query, now_ms);
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
        (cat, models)
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
        self.rebuild(ctx, not_now, novelty, now_ms);
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            match self.pick(novelty) {
                Some(t) => out.push(self.cat.id[t as usize]),
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
        let cfg = &self.cfg;
        let e = &cfg.engine;
        let pw = &cfg.profile.weights;
        // The event weight goes to taste outside an event window (§5.2); no event yet. Season
        // pick and influence of «Настроить волну» rescale the weights (T3.18).
        let [w_taste, w_context, w_session, w_season, w_filters] = QueryState::weights(
            &self.query,
            [
                pw.taste + pw.event,
                pw.context,
                pw.session,
                pw.season,
                pw.filters,
            ],
        );
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
        let w_ctx = context_weights(&self.context, &sctx, e.context_prior_n);
        let w_sess = self.session_vec;
        let sigma = e.taste_sigma * (0.5 + novelty);
        let wt = self.taste.sample(&mut self.rng, sigma);
        let bias = self.taste.b;
        let repeat_ms = i64::from(cfg.profile.sequencing.repeat_window_h) * 3600 * 1000;
        let soft_ms = i64::from(e.repeat_soft_days) * DAY_MS;
        // «Не сейчас» in this context bucket blocks for its shelf time (D-77).
        self.blocked.fill(false);
        // Already queued in this session — never twice (the top K must not fill with them).
        for &q in &self.session.queued {
            if let Some(i) = self.cat.idx(q) {
                self.blocked[i] = true;
            }
        }
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
        let mut sums = [0f32; 4];
        let mut counted = 0u32;
        let cat = &self.cat;
        let fam = &e.familiarity;
        let qs = &self.qs;
        for t in 0..cat.len() {
            // Layer 1: «никогда», disliked, «не сейчас», the hard repeat window; layer 3:
            // «Настроить волну».
            if !qs.pass[t]
                || cat.never[t]
                || self.blocked[t]
                || cat.mean(t) < fam.never_return_mean
                || now_ms - cat.last_played[t] < repeat_ms
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
            let x = &cat.feat[t];
            let taste = 0.6 * sigmoid(dot(&wt, x) + bias) + 0.4 * cat.mean(t);
            let context = sigmoid(dot(&w_ctx, x));
            let session = 0.5 + 0.5 * dot(&w_sess, x).tanh();
            let mut s = w_taste * taste
                + w_context * context
                + w_session * session
                + w_season * season
                + w_filters * qs.filters[t];
            sums[0] += w_taste * taste;
            sums[1] += w_context * context;
            sums[2] += w_session * session;
            sums[3] += w_season * season;
            counted += 1;
            let g = cat.group(t, now_ms, fam);
            if g == 2 && novelty > 0.0 {
                let mut d2 = 0.0;
                for (i, v) in x.iter().enumerate() {
                    let d = v - cat.center[i];
                    d2 += d * d;
                }
                s += e.novelty_pull * novelty * (d2 / D as f32).sqrt();
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
        let k = counted.max(1) as f32;
        self.last = Explained {
            wt,
            bias,
            w_ctx,
            w_sess,
            w: [w_taste, w_context, w_session, w_season],
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
        let mut parts = [
            (l.w[0] * taste - l.mean[0], 0usize),
            (l.w[1] * context - l.mean[1], 1),
            (l.w[2] * session - l.mean[2], 2),
            (l.w[3] * season - l.mean[3], 3),
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
                _ => Why::Season(dominant(&l.season_w) as u8),
            });
        }
        if out.is_empty() {
            out.push(Why::Taste);
        }
        out
    }

    /// Stage 2: the group furthest behind its quota → Thompson + constraints → softmax.
    fn pick(&mut self, novelty: f32) -> Option<u32> {
        let e = &self.cfg.engine;
        let seq = &self.cfg.profile.sequencing;
        let q = self.cfg.quotas(novelty);
        let ss = &mut self.session;
        let deficit = |g: usize| q[g] * (ss.slots + 1) as f32 - ss.group_counts[g] as f32;
        let mut order = [0usize, 1, 2];
        order.sort_by(|&a, &b| deficit(b).total_cmp(&deficit(a)));
        let explore = e.explore_base + e.explore_slope * novelty;
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
                        if ss.queued.contains(&cat.id[t])
                            || ss.recent_artists.contains(&cat.artist[t])
                        {
                            continue;
                        }
                        let x = &cat.feat[t];
                        if let Some(pe) = ss.prev_energy
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
                if ss.recent_artists.len() > seq.artist_spacing.saturating_sub(1) {
                    ss.recent_artists.remove(0);
                }
                ss.prev_energy = Some(cat.feat[t][f::ENERGY]);
                ss.prev_tempo = Some(cat.feat[t][f::TEMPO]);
                ss.group_counts[cg] += 1;
                ss.slots += 1;
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
}

#[derive(Default)]
struct Explained {
    wt: [f32; D],
    bias: f32,
    w_ctx: [f32; D],
    w_sess: [f32; D],
    w: [f32; 4],
    mean: [f32; 4],
    season_w: [f32; 4],
    daypart: u8,
    now_ms: i64,
}
