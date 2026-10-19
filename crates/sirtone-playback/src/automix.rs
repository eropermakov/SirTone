//! The automix planner (T4.8, FR-PLAY-010, `04` §4.3 — copy of `01a` §A: Mixxx Skip Silence,
//! Plexamp/MPD/RadioBOSS envelope overlap, Rockbox mix-vs-crossfade, VirtualDJ/djay beatmatch).
//! Pure: the player reads what the analysis knows about two tracks, this decides how A hands over
//! to B — the ladder L0 (gapless) → L1 (silence trimmed) → L2 (envelope overlap) → L3
//! (beatmatch), always the highest level both tracks allow. The engine only executes the plan.
//! All numbers: `assets/automix/automix.json`.

use serde::Deserialize;

/// What the analysis knows about a track (B0 envelopes and silences, B1 tempo/key, B3 energy).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MixTrack {
    pub duration_ms: u64,
    /// `None` — not analysed yet.
    pub lead_silence_ms: Option<u64>,
    pub tail_silence_ms: Option<u64>,
    pub lufs: Option<f32>,
    /// 10 Hz power envelopes of the first / last 30 s, `dB + 100` (B0).
    pub env_head: Vec<u8>,
    pub env_tail: Vec<u8>,
    /// The genre tag as written.
    pub genre: Option<String>,
    pub bpm: Option<f32>,
    /// Camelot key, `8A`…; `None` — unknown.
    pub camelot: Option<String>,
    pub energy: Option<f32>,
    /// Beat grids of the first and the last 30 s (T4.9c, D-100); `None` until measured.
    pub head_grid: Option<BeatGrid>,
    pub tail_grid: Option<BeatGrid>,
}

/// A beat grid over a window: the tempo, one beat (ms in the track), how steady (0…1).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BeatGrid {
    pub bpm: f32,
    pub beat_ms: f64,
    pub stability: f32,
}

impl MixTrack {
    fn analysed(&self) -> bool {
        self.lead_silence_ms.is_some()
            && self.tail_silence_ms.is_some()
            && self.lufs.is_some()
            && !self.env_head.is_empty()
            && !self.env_tail.is_empty()
    }

    /// Where the sound of the track ends (its tail silence cut off).
    fn audio_end_ms(&self) -> u64 {
        self.duration_ms
            .saturating_sub(self.tail_silence_ms.unwrap_or(0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Intensity {
    Soft,
    Smart,
    Dj,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Gapless,
    /// A fixed crossfade, seconds 1–12.
    Crossfade(f32),
    Automix(Intensity),
}

/// The settings that shape a plan.
#[derive(Debug, Clone, PartialEq)]
pub struct MixCfg {
    pub mode: Mode,
    pub skip_albums: bool,
    pub genre_blacklist: Vec<String>,
}

/// Where the two tracks come from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MixCtx {
    /// An album playing in order (the same predicate as album gain: from an album, no shuffle)
    /// and both tracks belong to it.
    pub album_in_order: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IntensityParams {
    pub min_ms: u64,
    pub max_ms: u64,
    pub tempo_tolerance: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NaturalFade {
    pub drop_db: f32,
    pub window_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BeatmatchParams {
    pub grid_min: f32,
    pub camelot_max: u32,
    pub beats_min: u32,
    pub beats_max: u32,
    pub blend_energy: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MixNowParams {
    pub min_lead_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManualParams {
    pub fade_out_ms: u64,
    pub fade_in_ms: u64,
}

/// `assets/automix/automix.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct MixParams {
    pub threshold_db: f32,
    pub natural_fade: NaturalFade,
    pub sharp_start_ms: u64,
    pub intensity: std::collections::HashMap<String, IntensityParams>,
    pub beatmatch: BeatmatchParams,
    pub manual: ManualParams,
    pub mix_now: MixNowParams,
    pub genre_blacklist_words: std::collections::HashMap<String, Vec<String>>,
}

impl MixParams {
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// The longest overlap a mode allows (crossfade: its length).
    pub fn max_overlap_ms(&self, mode: Mode) -> Option<u64> {
        match mode {
            Mode::Gapless => None,
            Mode::Crossfade(s) => Some((s.clamp(1.0, 12.0) * 1000.0) as u64),
            Mode::Automix(i) => self.of(i).map(|p| p.max_ms),
        }
    }

    fn of(&self, i: Intensity) -> Option<&IntensityParams> {
        self.intensity.get(match i {
            Intensity::Soft => "soft",
            Intensity::Smart => "smart",
            Intensity::Dj => "dj",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Gapless, nothing changed.
    L0,
    /// Silences trimmed, joint without overlap.
    L1,
    /// Overlap by the envelopes (or the fixed crossfade).
    L2,
    /// Beatmatched.
    L3,
}

/// How a deck's volume moves over the overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// Untouched (a natural fade-out mixed under, a soft start).
    Keep,
    /// Equal power: `cos(πt/2)` out, `sin(πt/2)` in (−3 dB in the middle).
    EqualPower,
}

/// The handover from A to B.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub level: Level,
    /// In A: B starts sounding here …
    pub a_out_ms: u64,
    /// … and A stops here (`a_out_ms` = `a_stop_ms` — no overlap).
    pub a_stop_ms: u64,
    /// Where B starts (its lead silence skipped).
    pub b_start_ms: u64,
    pub a_curve: Curve,
    pub b_curve: Curve,
    /// How long the overlap sounds (output time). Without a stretch it is `a_stop − a_out`.
    pub out_ms: u64,
    /// L3: A's playback rate held after the ramp (input per output; 1 — no stretch) …
    pub rate: f64,
    /// … reached from 1 over this long at the start of the overlap.
    pub ramp_ms: u64,
    /// Why this level (diagnostics, `explain`).
    pub reason: &'static str,
}

impl Plan {
    pub fn overlap_ms(&self) -> u64 {
        self.out_ms
    }

    fn gapless(a: &MixTrack, reason: &'static str) -> Self {
        Self {
            level: Level::L0,
            a_out_ms: a.duration_ms,
            a_stop_ms: a.duration_ms,
            b_start_ms: 0,
            a_curve: Curve::Keep,
            b_curve: Curve::Keep,
            out_ms: 0,
            rate: 1.0,
            ramp_ms: 0,
            reason,
        }
    }
}

/// A genre tag that one of the blacklist keys forbids (word match, aliases from the params).
pub fn genre_blocked(genre: Option<&str>, blacklist: &[String], p: &MixParams) -> bool {
    let Some(g) = genre.map(str::to_lowercase) else {
        return false;
    };
    blacklist.iter().any(|key| {
        p.genre_blacklist_words.get(key).map_or_else(
            || g.contains(key.as_str()),
            |w| w.iter().any(|w| g.contains(w.as_str())),
        )
    })
}

/// The time from the last envelope window at or above `thr` to the end of the sound, ms (the
/// tail's quiet part); the envelope's last window ends at `duration`.
fn quiet_tail_ms(t: &MixTrack, thr: f32) -> u64 {
    let n = t.env_tail.len() as u64;
    let audio_end = t.audio_end_ms();
    let env_start = t.duration_ms.saturating_sub(n * 100);
    match t
        .env_tail
        .iter()
        .rposition(|v| f32::from(*v) - 100.0 >= thr)
    {
        Some(i) => audio_end.saturating_sub(env_start + (i as u64 + 1) * 100),
        None => 0,
    }
}

/// The time from B's first sound to its first window at or above `thr`, ms (the quiet intro).
fn quiet_head_ms(t: &MixTrack, thr: f32) -> u64 {
    let lead = t.lead_silence_ms.unwrap_or(0);
    match t.env_head.iter().position(|v| f32::from(*v) - 100.0 >= thr) {
        Some(i) => (i as u64 * 100).saturating_sub(lead),
        None => 0,
    }
}

/// The tail falls on its own: ≥ `drop_db` over the last `window_ms` of sound.
fn fades_naturally(t: &MixTrack, p: &MixParams) -> bool {
    let n = t.env_tail.len();
    let tail_silence_windows = (t.tail_silence_ms.unwrap_or(0) / 100) as usize;
    let end = n.saturating_sub(tail_silence_windows);
    let w = (p.natural_fade.window_ms / 100) as usize;
    if end < w + 1 {
        return false;
    }
    let start = f32::from(t.env_tail[end - w]);
    let last = t.env_tail[end - 3..end]
        .iter()
        .map(|v| f32::from(*v))
        .fold(f32::MIN, f32::max);
    start - last >= p.natural_fade.drop_db
}

fn camelot_distance(a: &str, b: &str) -> Option<u32> {
    let parse = |s: &str| {
        let (n, l) = s.split_at(s.len().checked_sub(1)?);
        Some((n.parse::<i32>().ok()?, l.to_ascii_uppercase()))
    };
    let (na, la) = parse(a)?;
    let (nb, lb) = parse(b)?;
    let d = (na - nb).rem_euclid(12).min((nb - na).rem_euclid(12)) as u32;
    Some(if la == lb { d } else { d + 1 })
}

/// Tempo ratio B/A brought to the nearest of ×1, ×2, ×½.
fn tempo_ratio(a: f32, b: f32) -> f32 {
    let r = b / a;
    [r, r * 2.0, r / 2.0]
        .into_iter()
        .min_by(|x, y| (x - 1.0).abs().total_cmp(&(y - 1.0).abs()))
        .unwrap_or(r)
}

/// Can A and B be beatmatched (`04` §4.3 L3)? Grids steady, tempi within the tolerance, keys
/// compatible.
pub fn beatmatch_ok(a: &MixTrack, b: &MixTrack, tolerance: f32, p: &MixParams) -> bool {
    let (Some(ga), Some(gb)) = (a.tail_grid.as_ref(), b.head_grid.as_ref()) else {
        return false;
    };
    if tolerance <= 0.0
        || ga.stability < p.beatmatch.grid_min
        || gb.stability < p.beatmatch.grid_min
        || ga.bpm <= 0.0
        || gb.bpm <= 0.0
    {
        return false;
    }
    let keys = match (a.camelot.as_deref(), b.camelot.as_deref()) {
        (Some(x), Some(y)) => camelot_distance(x, y).is_some_and(|d| d <= p.beatmatch.camelot_max),
        _ => false,
    };
    keys && (tempo_ratio(ga.bpm, gb.bpm) - 1.0).abs() <= tolerance
}

/// The plan for A → B.
pub fn plan(a: &MixTrack, b: &MixTrack, ctx: MixCtx, cfg: &MixCfg, p: &MixParams) -> Plan {
    // L0: no mixing at all.
    if cfg.mode == Mode::Gapless {
        return Plan::gapless(a, "mode: gapless");
    }
    if ctx.album_in_order && cfg.skip_albums {
        return Plan::gapless(a, "album in order");
    }
    if genre_blocked(a.genre.as_deref(), &cfg.genre_blacklist, p)
        || genre_blocked(b.genre.as_deref(), &cfg.genre_blacklist, p)
    {
        return Plan::gapless(a, "genre not mixed");
    }
    let analysed = a.analysed() && b.analysed();
    // A fixed crossfade: on the natural ends without analysis, on the sound with it.
    if let Mode::Crossfade(secs) = cfg.mode {
        let ov = (secs.clamp(1.0, 12.0) * 1000.0) as u64;
        let a_stop = if analysed {
            a.audio_end_ms()
        } else {
            a.duration_ms
        };
        let ov = ov.min(a_stop / 2);
        return Plan {
            level: Level::L2,
            a_out_ms: a_stop - ov,
            a_stop_ms: a_stop,
            b_start_ms: if analysed {
                b.lead_silence_ms.unwrap_or(0)
            } else {
                0
            },
            a_curve: Curve::EqualPower,
            b_curve: Curve::EqualPower,
            out_ms: ov,
            rate: 1.0,
            ramp_ms: 0,
            reason: "mode: crossfade",
        };
    }
    let Mode::Automix(intensity) = cfg.mode else {
        return Plan::gapless(a, "mode");
    };
    if !analysed {
        return Plan::gapless(a, "not analysed yet");
    }
    let Some(ip) = p.of(intensity) else {
        return Plan::gapless(a, "no params");
    };
    // L1: silences trimmed.
    let a_stop = a.audio_end_ms();
    let b_start = b.lead_silence_ms.unwrap_or(0);
    // L2: overlap where A is quiet at its end and B at its start.
    let thr_a = a.lufs.unwrap_or(-14.0) - p.threshold_db;
    let thr_b = b.lufs.unwrap_or(-14.0) - p.threshold_db;
    let want = quiet_tail_ms(a, thr_a) + quiet_head_ms(b, thr_b);
    let ov = want.clamp(ip.min_ms, ip.max_ms).min(a_stop / 2);
    if ov == 0 {
        return Plan {
            level: Level::L1,
            a_out_ms: a_stop,
            a_stop_ms: a_stop,
            b_start_ms: b_start,
            a_curve: Curve::Keep,
            b_curve: Curve::Keep,
            out_ms: 0,
            rate: 1.0,
            ramp_ms: 0,
            reason: "silence trimmed",
        };
    }
    let a_curve = if fades_naturally(a, p) {
        Curve::Keep
    } else {
        Curve::EqualPower
    };
    let b_curve = if quiet_head_ms(b, thr_b) <= p.sharp_start_ms {
        Curve::EqualPower
    } else {
        Curve::Keep
    };
    let l2 = Plan {
        level: Level::L2,
        a_out_ms: a_stop - ov,
        a_stop_ms: a_stop,
        b_start_ms: b_start,
        a_curve,
        b_curve,
        out_ms: ov,
        rate: 1.0,
        ramp_ms: 0,
        reason: "envelope overlap",
    };
    if beatmatch_ok(a, b, ip.tempo_tolerance, p)
        && let Some(l3) = beatmatch(a, b, &l2, p)
    {
        return l3;
    }
    l2
}

/// A's input consumed after `u` ms of output with the rate ramping 1 → `r` over `ramp`, then
/// held at `r`.
fn consumed(u: f64, r: f64, ramp: f64) -> f64 {
    if u <= ramp {
        u + (r - 1.0) * u * u / (2.0 * ramp)
    } else {
        ramp * (1.0 + r) / 2.0 + (u - ramp) * r
    }
}

/// L3 (`04` §4.3, VirtualDJ «tempo» / djay «Sync + Tempo blend»): A, entering on one of its
/// beats, is pulled to B's tempo over about a bar and held there; the overlap is whole bars of B;
/// B starts at the point that puts its beats on A's. `None` — it does not fit before A's end.
fn beatmatch(a: &MixTrack, b: &MixTrack, l2: &Plan, p: &MixParams) -> Option<Plan> {
    let (ga, gb) = (a.tail_grid?, b.head_grid?);
    let pb = 60_000.0 / f64::from(gb.bpm);
    // A's beat period in its own time, at the octave nearest B's.
    let pa = [1.0, 2.0, 0.5]
        .map(|f| 60_000.0 / f64::from(ga.bpm) * f)
        .into_iter()
        .min_by(|x, y| ((x / pb) - 1.0).abs().total_cmp(&((y / pb) - 1.0).abs()))?;
    let r = pa / pb;
    let end = a.audio_end_ms() as f64;
    let bars = (l2.out_ms as f64 / pb / 4.0).round() as u32;
    let (lo, hi) = (p.beatmatch.beats_min, p.beatmatch.beats_max);
    let beats = (bars * 4).clamp(lo, hi);
    let out = f64::from(beats) * pb;
    let ramp = (4.0 * pb).min(out / 2.0);
    // A enters on its last beat that still lets the overlap end with A's sound.
    let latest = end - consumed(out, r, ramp);
    let a_out = ga.beat_ms + ((latest - ga.beat_ms) / pa).floor() * pa;
    if a_out < end / 2.0 {
        return None;
    }
    // The first A beat after the ramp and its output time; B starts so that its beat is there.
    let m = (consumed(ramp, r, ramp) / pa).ceil();
    let u_m = ramp + (m * pa - ramp * (1.0 + r) / 2.0) / r;
    let lead = b.lead_silence_ms.unwrap_or(0) as f64;
    let mut b_start = (gb.beat_ms - u_m).rem_euclid(pb);
    while b_start < lead {
        b_start += pb;
    }
    Some(Plan {
        level: Level::L3,
        a_out_ms: a_out.round() as u64,
        a_stop_ms: (a_out + consumed(out, r, ramp)).round() as u64,
        b_start_ms: b_start.round() as u64,
        a_curve: Curve::EqualPower,
        b_curve: Curve::EqualPower,
        out_ms: out.round() as u64,
        rate: r,
        ramp_ms: ramp.round() as u64,
        reason: "beatmatch",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> MixParams {
        MixParams::from_json(include_str!("../../../assets/automix/automix.json")).unwrap()
    }

    fn cfg(mode: Mode) -> MixCfg {
        MixCfg {
            mode,
            skip_albums: true,
            genre_blacklist: ["classical", "live", "ambient", "audiobook"]
                .map(String::from)
                .to_vec(),
        }
    }

    /// A 3-minute track at −10 LUFS: loud (−8 dB windows) except `quiet_tail_s` of fade at the end
    /// (−40 dB) and `quiet_head_s` of quiet intro.
    fn track(quiet_head_s: u64, quiet_tail_s: u64, tail_silence_ms: u64) -> MixTrack {
        let loud = 92u8; // −8 dB
        let quiet = 60u8; // −40 dB
        let mut head = vec![loud; 300];
        for v in head.iter_mut().take((quiet_head_s * 10) as usize) {
            *v = quiet;
        }
        let mut tail = vec![loud; 300];
        let sil = (tail_silence_ms / 100) as usize;
        let q = (quiet_tail_s * 10) as usize;
        for v in tail[300 - sil - q..300 - sil].iter_mut() {
            *v = quiet;
        }
        for v in tail[300 - sil..].iter_mut() {
            *v = 0;
        }
        MixTrack {
            duration_ms: 180_000,
            lead_silence_ms: Some(0),
            tail_silence_ms: Some(tail_silence_ms),
            lufs: Some(-10.0),
            env_head: head,
            env_tail: tail,
            ..MixTrack::default()
        }
    }

    #[test]
    fn the_ladder() {
        let p = params();
        let smart = cfg(Mode::Automix(Intensity::Smart));
        let ctx = MixCtx::default();
        // Gapless mode → L0 at the natural end.
        let a = track(0, 3, 1_000);
        let b = track(2, 0, 0);
        let g = plan(&a, &b, ctx, &cfg(Mode::Gapless), &p);
        assert_eq!(
            (g.level, g.a_out_ms, g.overlap_ms()),
            (Level::L0, 180_000, 0)
        );
        // Automix: 3 s quiet tail + 2 s quiet intro → 5 s overlap, ending where A's sound ends.
        let m = plan(&a, &b, ctx, &smart, &p);
        assert_eq!(m.level, Level::L2);
        assert_eq!(m.a_stop_ms, 179_000);
        assert_eq!(m.overlap_ms(), 5_000);
        // Bounded: «Мягко» at most 3 s; «Умно» at least 2.5 s even with nothing quiet.
        let soft = plan(&a, &b, ctx, &cfg(Mode::Automix(Intensity::Soft)), &p);
        assert_eq!(soft.overlap_ms(), 3_000);
        let hard = plan(&track(0, 0, 0), &track(0, 0, 0), ctx, &smart, &p);
        assert_eq!(hard.overlap_ms(), 2_500);
        // «Мягко» with nothing quiet: silence trimmed only (L1).
        let l1 = plan(
            &track(0, 0, 2_000),
            &track(0, 0, 0),
            ctx,
            &cfg(Mode::Automix(Intensity::Soft)),
            &p,
        );
        assert_eq!(
            (l1.level, l1.a_stop_ms, l1.overlap_ms()),
            (Level::L1, 178_000, 0)
        );
        // Crossfade mode: exactly the seconds, equal power both ways.
        let x = plan(&a, &b, ctx, &cfg(Mode::Crossfade(6.0)), &p);
        assert_eq!((x.level, x.overlap_ms()), (Level::L2, 6_000));
        assert_eq!(
            (x.a_curve, x.b_curve),
            (Curve::EqualPower, Curve::EqualPower)
        );
    }

    #[test]
    fn the_exceptions() {
        let p = params();
        let smart = cfg(Mode::Automix(Intensity::Smart));
        let a = track(0, 3, 0);
        let b = track(0, 0, 0);
        // An album in order → L0.
        let album = plan(
            &a,
            &b,
            MixCtx {
                album_in_order: true,
            },
            &smart,
            &p,
        );
        assert_eq!((album.level, album.reason), (Level::L0, "album in order"));
        // … unless the setting is off.
        let mut keep = smart.clone();
        keep.skip_albums = false;
        assert_eq!(
            plan(
                &a,
                &b,
                MixCtx {
                    album_in_order: true
                },
                &keep,
                &p
            )
            .level,
            Level::L2
        );
        // Blacklisted genres, in either track, Russian tags too.
        for g in [
            "Classical",
            "Modern Classical",
            "Эмбиент",
            "Live",
            "Аудиокнига",
        ] {
            let mut c = b.clone();
            c.genre = Some(g.into());
            assert_eq!(
                plan(&a, &c, MixCtx::default(), &smart, &p).level,
                Level::L0,
                "{g}"
            );
        }
        let mut pop = b.clone();
        pop.genre = Some("Pop".into());
        assert_eq!(
            plan(&a, &pop, MixCtx::default(), &smart, &p).level,
            Level::L2
        );
        // Not analysed: automix → L0; crossfade → the fixed crossfade on the natural ends.
        let raw = MixTrack {
            duration_ms: 200_000,
            ..MixTrack::default()
        };
        assert_eq!(
            plan(&raw, &b, MixCtx::default(), &smart, &p).level,
            Level::L0
        );
        let x = plan(&raw, &b, MixCtx::default(), &cfg(Mode::Crossfade(4.0)), &p);
        assert_eq!(
            (x.a_stop_ms, x.overlap_ms(), x.b_start_ms),
            (200_000, 4_000, 0)
        );
    }

    #[test]
    fn mix_under_a_natural_fade_and_fade_in_a_sharp_start() {
        let p = params();
        let smart = cfg(Mode::Automix(Intensity::Smart));
        // A falls by 32 dB over its last seconds: mixed under, A untouched.
        let fading = plan(
            &track(0, 3, 0),
            &track(3, 0, 0),
            MixCtx::default(),
            &smart,
            &p,
        );
        assert_eq!(fading.a_curve, Curve::Keep);
        // A loud to the end: crossfaded.
        let abrupt = plan(
            &track(0, 0, 0),
            &track(0, 0, 0),
            MixCtx::default(),
            &smart,
            &p,
        );
        assert_eq!(abrupt.a_curve, Curve::EqualPower);
        // B starts loud at once → fade-in; a 3 s quiet intro → no fade needed.
        assert_eq!(abrupt.b_curve, Curve::EqualPower);
        assert_eq!(fading.b_curve, Curve::Keep);
    }

    #[test]
    fn beatmatch_only_with_steady_grids_close_tempi_and_keys() {
        let p = params();
        let grid = |bpm: f32, stability: f32| BeatGrid {
            bpm,
            beat_ms: 0.0,
            stability,
        };
        let set = |t: &mut MixTrack, g: BeatGrid| {
            t.head_grid = Some(g);
            t.tail_grid = Some(g);
        };
        let mut a = track(0, 0, 0);
        let mut b = track(0, 0, 0);
        set(&mut a, grid(124.0, 0.9));
        set(&mut b, grid(128.0, 0.9));
        a.camelot = Some("8A".into());
        b.camelot = Some("9A".into());
        let smart = cfg(Mode::Automix(Intensity::Smart));
        assert_eq!(plan(&a, &b, MixCtx::default(), &smart, &p).level, Level::L3);
        // Half tempo counts as the same (×2/÷2).
        set(&mut b, grid(64.0, 0.9));
        assert_eq!(plan(&a, &b, MixCtx::default(), &smart, &p).level, Level::L3);
        // An unsteady grid → L2.
        set(&mut b, grid(128.0, 0.5));
        assert_eq!(plan(&a, &b, MixCtx::default(), &smart, &p).level, Level::L2);
        // Too far apart for «Умно» (6 %), close enough for «Как диджей» (10 %).
        set(&mut b, grid(135.0, 0.9));
        assert_eq!(plan(&a, &b, MixCtx::default(), &smart, &p).level, Level::L2);
        let dj = cfg(Mode::Automix(Intensity::Dj));
        assert_eq!(plan(&a, &b, MixCtx::default(), &dj, &p).level, Level::L3);
        // Clashing keys → L2; «Мягко» never beatmatches.
        set(&mut b, grid(128.0, 0.9));
        b.camelot = Some("3B".into());
        assert_eq!(plan(&a, &b, MixCtx::default(), &smart, &p).level, Level::L2);
        b.camelot = Some("8B".into());
        assert_eq!(plan(&a, &b, MixCtx::default(), &smart, &p).level, Level::L3);
        let soft = cfg(Mode::Automix(Intensity::Soft));
        assert_ne!(plan(&a, &b, MixCtx::default(), &soft, &p).level, Level::L3);
        // No grid at all (today's analysis) → L2.
        a.tail_grid = None;
        assert_eq!(plan(&a, &b, MixCtx::default(), &smart, &p).level, Level::L2);
    }

    /// L3: after the ramp every A beat (in output time) falls on a B beat; A enters on a beat;
    /// the overlap is whole bars of B and ends with A's sound.
    #[test]
    fn beatmatched_beats_coincide() {
        let p = params();
        for (ta, tb, pa_ms, pb_ms) in [
            (124.0f32, 128.0f32, 37.0, 210.0),
            (90.0, 92.0, 100.0, 0.0),
            (174.0, 87.5, 5.0, 300.0),
        ] {
            let mut a = track(0, 0, 0);
            let mut b = track(0, 0, 0);
            a.tail_grid = Some(BeatGrid {
                bpm: ta,
                beat_ms: 150_000.0 + pa_ms,
                stability: 0.9,
            });
            b.head_grid = Some(BeatGrid {
                bpm: tb,
                beat_ms: pb_ms,
                stability: 0.9,
            });
            a.camelot = Some("8A".into());
            b.camelot = Some("8A".into());
            let pl = plan(
                &a,
                &b,
                MixCtx::default(),
                &cfg(Mode::Automix(Intensity::Dj)),
                &p,
            );
            assert_eq!(pl.level, Level::L3, "{ta}→{tb}");
            let pb = 60_000.0 / f64::from(tb);
            let pa = [1.0, 2.0, 0.5]
                .map(|f| 60_000.0 / f64::from(ta) * f)
                .into_iter()
                .min_by(|x, y| ((x / pb) - 1.0).abs().total_cmp(&((y / pb) - 1.0).abs()))
                .unwrap();
            // A enters on its beat.
            let ph = ((pl.a_out_ms as f64 - (150_000.0 + pa_ms)) / pa)
                .fract()
                .abs();
            assert!(ph.min(1.0 - ph) * pa < 1.0, "entry off the beat");
            // Whole bars of B.
            let bars = pl.out_ms as f64 / pb / 4.0;
            assert!((bars - bars.round()).abs() < 0.01, "{bars}");
            assert!(pl.a_stop_ms <= 180_000);
            let r = pl.rate;
            let ramp = pl.ramp_ms as f64;
            let mut checked = 0;
            for m in 1.. {
                let input = m as f64 * pa;
                if input <= consumed(ramp, r, ramp) {
                    continue;
                }
                let u = ramp + (input - ramp * (1.0 + r) / 2.0) / r;
                if u > pl.out_ms as f64 {
                    break;
                }
                let at_b = pl.b_start_ms as f64 + u - pb_ms;
                let off = (at_b / pb - (at_b / pb).round()).abs() * pb;
                assert!(off < 2.0, "{ta}→{tb}: beat {m} off by {off:.1} ms");
                checked += 1;
            }
            assert!(checked >= 4, "{checked}");
        }
    }
}
