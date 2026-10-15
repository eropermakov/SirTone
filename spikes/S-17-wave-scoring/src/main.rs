//! S-17 — прототип скоринга движка волны (docs/14_WAVE_ENGINE.md).
//!
//! Что проверяет:
//! 1. Этап 1 (кандидаты → скоринг): все компоненты профиля по матрице признаков в памяти
//!    на синтетической медиатеке (по умолчанию 50 000 треков) — цель < 15 мс.
//! 2. Этап 2 (секвенсор): квоты ползунка «Знакомое ↔ Неожиданное», Томпсон-сэмплинг,
//!    разнос исполнителей, плавность энергии, бонус перехода для автомикса, выбор из топ-20.
//! 3. Обновление модели вкуса после события (онлайн-логистическая регрессия с диагональным
//!    приближением Лапласа, как у Chapelle & Li 2011) — цель «микросекунды».
//! 4. Функциональные проверки: пропорции ползунка, «никогда в волне», разнос исполнителей,
//!    кривая энергии засыпательной волны.
//!
//! Запуск: `cargo run --release -- [N=50000] [итераций=300]`, тесты календаря: `cargo test --release`.

mod calendar;
mod rng;

use rng::Rng;
use std::time::Instant;

const D: usize = 32;
const F_ENERGY: usize = 0;
const F_VALENCE: usize = 1;
const F_DANCE: usize = 2;
const F_TEMPO: usize = 3; // (bpm − 60) / 140
const F_KEY_SIN: usize = 4;
const F_KEY_COS: usize = 5;
const F_MAJOR: usize = 6;
const F_ACOUSTIC: usize = 7;
const F_VOCAL: usize = 8;
const F_BRIGHT: usize = 9;
const F_DYN: usize = 10;
const F_AGGR: usize = 14;
const F_AROUSAL: usize = 15;
const F_MOOD: usize = 16; // 8 тегов: happy, upbeat, calm, sad, aggressive, tender, scary, festive
const F_GENRE: usize = 24; // 8 жанров (one-hot)

const TOP_K: usize = 64; // кандидатов на группу знакомости после этапа 1
const TOP_SAMPLE: usize = 20; // из скольких выбирает секвенсор
const ARTIST_SPACING: usize = 5;
const MAX_ENERGY_JUMP: f32 = 0.35;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fam {
    Loved,
    Familiar,
    Forgotten,
    New,
    Untouched,
}

/// Три группы квот ползунка: 0 — любимое и знакомое, 1 — забытое, 2 — новое и нетронутое.
fn group_of(f: Fam) -> usize {
    match f {
        Fam::Loved | Fam::Familiar => 0,
        Fam::Forgotten => 1,
        Fam::New | Fam::Untouched => 2,
    }
}

/// Таблица владельца (§34): позиция ползунка → доли групп; между засечками — линейно.
fn quotas(novelty: f32) -> [f32; 3] {
    const TABLE: [(f32, [f32; 3]); 5] = [
        (0.00, [0.90, 0.10, 0.00]),
        (0.25, [0.70, 0.15, 0.15]),
        (0.50, [0.50, 0.20, 0.30]),
        (0.75, [0.30, 0.20, 0.50]),
        (1.00, [0.15, 0.15, 0.70]),
    ];
    let v = novelty.clamp(0.0, 1.0);
    for w in TABLE.windows(2) {
        let (a, qa) = w[0];
        let (b, qb) = w[1];
        if v <= b {
            let t = (v - a) / (b - a);
            return std::array::from_fn(|i| qa[i] + t * (qb[i] - qa[i]));
        }
    }
    TABLE[4].1
}

#[inline(always)]
fn dot(a: &[f32; D], b: &[f32; D]) -> f32 {
    let mut acc = [0.0f32; 8];
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

struct Library {
    feat: Vec<[f32; D]>,
    artist: Vec<u32>,
    season_aff: Vec<[f32; 4]>,
    event_aff: Vec<f32>,
    universal: Vec<f32>,
    alpha: Vec<f32>,
    beta: Vec<f32>,
    plays: Vec<u16>,
    last_played: Vec<i32>,
    added: Vec<i32>,
    liked: Vec<bool>,
    never: Vec<bool>,
}

impl Library {
    fn synth(n: usize, today: i32, rng: &mut Rng) -> Self {
        let mut lib = Library {
            feat: Vec::with_capacity(n),
            artist: Vec::with_capacity(n),
            season_aff: Vec::with_capacity(n),
            event_aff: Vec::with_capacity(n),
            universal: Vec::with_capacity(n),
            alpha: Vec::with_capacity(n),
            beta: Vec::with_capacity(n),
            plays: Vec::with_capacity(n),
            last_played: Vec::with_capacity(n),
            added: Vec::with_capacity(n),
            liked: Vec::with_capacity(n),
            never: Vec::with_capacity(n),
        };
        let n_artists = (n / 10).max(1) as u32;
        for _ in 0..n {
            let mut x = [0.0f32; D];
            let energy = rng.f32();
            let valence = rng.f32();
            x[F_ENERGY] = energy;
            x[F_VALENCE] = valence;
            x[F_DANCE] = (0.5 * energy + 0.5 * rng.f32()).min(1.0);
            let bpm = 70.0 + 110.0 * (0.6 * energy + 0.4 * rng.f32());
            x[F_TEMPO] = (bpm - 60.0) / 140.0;
            let key = rng.below(12) as f32 * std::f32::consts::TAU / 12.0;
            x[F_KEY_SIN] = 0.5 + 0.5 * key.sin();
            x[F_KEY_COS] = 0.5 + 0.5 * key.cos();
            x[F_MAJOR] = if rng.f32() < 0.4 + 0.4 * valence { 1.0 } else { 0.0 };
            x[F_ACOUSTIC] = ((1.0 - energy) * rng.f32()).min(1.0);
            x[F_VOCAL] = rng.f32();
            x[F_BRIGHT] = (0.3 + 0.5 * energy + 0.2 * rng.f32()).min(1.0);
            x[F_DYN] = rng.f32();
            for v in x.iter_mut().take(F_AGGR).skip(F_DYN + 1) {
                *v = rng.f32(); // громкость, длительность, год
            }
            let aggr = (energy * (1.0 - valence) * 1.6 * rng.f32()).min(1.0);
            x[F_AGGR] = aggr;
            x[F_AROUSAL] = (0.7 * energy + 0.3 * rng.f32()).min(1.0);
            let festive = if rng.f32() < 0.01 { 0.9 } else { 0.0 };
            let moods = [
                valence * energy,
                energy * (0.5 + 0.5 * valence),
                1.0 - energy,
                (1.0 - valence) * (1.0 - energy),
                aggr,
                valence * (1.0 - energy) * 0.8,
                (1.0 - valence) * rng.f32() * 0.6,
                festive,
            ];
            for (i, m) in moods.iter().enumerate() {
                x[F_MOOD + i] = m.clamp(0.0, 1.0);
            }
            x[F_GENRE + rng.below(8) as usize] = 1.0;

            let a = (rng.f32() * rng.f32() * n_artists as f32) as u32;
            lib.artist.push(a.min(n_artists - 1));
            let winter = 0.5 * (1.0 - energy) + 0.3 * x[F_ACOUSTIC] + 0.2 * rng.f32();
            let spring = 0.5 * valence + 0.3 * x[F_BRIGHT] + 0.2 * rng.f32();
            let summer = 0.4 * valence + 0.4 * x[F_DANCE] + 0.2 * rng.f32();
            let autumn = 0.4 * (1.0 - valence) + 0.4 * x[F_ACOUSTIC] + 0.2 * rng.f32();
            lib.season_aff.push([winter, spring, summer, autumn].map(|v| v.clamp(0.0, 1.0)));
            lib.event_aff.push(festive);
            let listeners = (rng.normal() * 1.2 + 6.0).exp();
            let pop = (listeners.ln() / 12.0).clamp(0.0, 1.0);
            let sound = 0.35 * valence + 0.25 * x[F_DANCE] + 0.2 * (1.0 - aggr) + 0.2 * x[F_VOCAL];
            lib.universal.push(0.6 * pop + 0.4 * sound);

            let r = rng.f32();
            let (plays, liked) = if r < 0.40 {
                (0u16, false)
            } else if r < 0.80 {
                (1 + rng.below(4) as u16, false)
            } else {
                (5 + rng.below(40) as u16, rng.f32() < 0.3)
            };
            lib.plays.push(plays);
            lib.liked.push(liked);
            lib.added.push(today - rng.below(5 * 365) as i32);
            lib.last_played
                .push(if plays > 0 { today - 1 - rng.below(2 * 365) as i32 } else { i32::MIN / 2 });
            let pos = plays as f32 * (0.5 + 0.5 * rng.f32());
            lib.alpha.push(1.0 + pos + if liked { 3.0 } else { 0.0 });
            lib.beta.push(1.0 + (plays as f32 - pos).max(0.0) + rng.below(3) as f32);
            lib.never.push(rng.f32() < 0.002);
            lib.feat.push(x);
        }
        lib
    }

    fn len(&self) -> usize {
        self.feat.len()
    }

    fn familiarity(&self, t: usize, today: i32) -> Fam {
        let plays = self.plays[t];
        let aff = self.alpha[t] / (self.alpha[t] + self.beta[t]);
        if self.liked[t] || (plays >= 8 && aff > 0.7) {
            Fam::Loved
        } else if plays >= 3 && today - self.last_played[t] > 60 {
            Fam::Forgotten
        } else if plays >= 1 {
            Fam::Familiar
        } else if today - self.added[t] <= 30 {
            Fam::New
        } else {
            Fam::Untouched
        }
    }

    fn bytes(&self) -> usize {
        let n = self.len();
        n * (D * 4 + 4 + 16 + 4 + 4 + 4 + 4 + 2 + 4 + 4 + 1 + 1)
    }
}

/// Веса компонентов профиля (§26); нормируются к сумме 1.
#[derive(Clone, Copy, Debug)]
struct Weights {
    taste: f32,
    session: f32,
    context: f32,
    season: f32,
    event: f32,
    curve: f32,
    universal: f32,
    filters: f32,
}

impl Weights {
    /// Стартовые веса из таблицы владельца; остаток до 1 делят session и filters поровну.
    fn profile(name: &str) -> Self {
        let (taste, context, season, event, curve, universal) = match name {
            "my_wave" => (0.45, 0.15, 0.10, 0.10, 0.0, 0.0),
            "seasonal" => (0.30, 0.10, 0.45, 0.05, 0.0, 0.0),
            "event" => (0.25, 0.05, 0.10, 0.55, 0.0, 0.0),
            "sleep" => (0.25, 0.05, 0.05, 0.0, 0.60, 0.0),
            "focus" => (0.30, 0.0, 0.05, 0.0, 0.50, 0.0),
            "for_all" => (0.05, 0.10, 0.05, 0.05, 0.0, 0.75),
            _ => (0.45, 0.15, 0.10, 0.0, 0.0, 0.0),
        };
        let rest = (1.0f32 - (taste + context + season + event + curve + universal)).max(0.0);
        Weights {
            taste,
            session: rest / 2.0,
            context,
            season,
            event,
            curve,
            universal,
            filters: rest / 2.0,
        }
    }
}

/// Снимок контекста (§26 ContextSnapshot) — собирается один раз на пересборку очереди.
struct Ctx {
    today: i32,
    season_w: [f32; 4],
    w_taste: [f32; D],
    b_taste: f32,
    w_ctx: [f32; D],
    w_session: [f32; D],
    center: [f32; D],
    target_energy: Option<f32>,
    curve_width: f32,
    event_active: bool,
    max_vocal: f32,
    max_aggr: f32,
    soft_mood: Option<usize>,
    novelty: f32,
    repeat_window_days: i32,
}

/// Модель вкуса: логистическая регрессия с диагональным гауссовым постериором
/// (среднее m, точность q) — обновление O(D), сэмплирование Томпсона O(D).
struct Taste {
    m: [f32; D],
    q: [f32; D],
    b: f32,
}

impl Taste {
    fn new() -> Self {
        Taste { m: [0.0; D], q: [1.0; D], b: 0.0 }
    }

    fn update(&mut self, x: &[f32; D], y: f32, weight: f32) {
        let p = sigmoid(dot(&self.m, x) + self.b);
        let g = weight * (y - p);
        let h = weight * p * (1.0 - p);
        for i in 0..D {
            self.q[i] += h * x[i] * x[i];
            self.m[i] += g * x[i] / self.q[i];
        }
        self.b += 0.05 * g;
    }

    fn sample(&self, rng: &mut Rng, scale: f32) -> [f32; D] {
        std::array::from_fn(|i| self.m[i] + scale * rng.normal() / self.q[i].sqrt())
    }
}

#[derive(Clone, Copy)]
struct Scored {
    score: f32,
    idx: u32,
}

#[inline]
fn score_track(lib: &Library, ctx: &Ctx, w: &Weights, t: usize) -> Option<(f32, usize)> {
    // слой 1–2: ручные исключения и жёсткие фильтры режима
    if lib.never[t] || ctx.today - lib.last_played[t] < ctx.repeat_window_days {
        return None;
    }
    let x = &lib.feat[t];
    if x[F_VOCAL] > ctx.max_vocal || x[F_AGGR] > ctx.max_aggr {
        return None;
    }
    // слой 4: мягкие веса профиля
    let aff = lib.alpha[t] / (lib.alpha[t] + lib.beta[t]);
    let taste = 0.6 * sigmoid(dot(&ctx.w_taste, x) + ctx.b_taste) + 0.4 * aff;
    let context = sigmoid(dot(&ctx.w_ctx, x));
    let session = 0.5 + 0.5 * dot(&ctx.w_session, x).tanh();
    let sa = &lib.season_aff[t];
    let season = ctx.season_w[0] * sa[0]
        + ctx.season_w[1] * sa[1]
        + ctx.season_w[2] * sa[2]
        + ctx.season_w[3] * sa[3];
    let event = if ctx.event_active { lib.event_aff[t] } else { 0.0 };
    let curve = match ctx.target_energy {
        Some(e) => 1.0 - ((x[F_ENERGY] - e).abs() / ctx.curve_width).min(1.0),
        None => 0.0,
    };
    let filters = match ctx.soft_mood {
        Some(m) => x[F_MOOD + m],
        None => 0.5,
    };
    let g = group_of(lib.familiarity(t, ctx.today));
    let mut s = w.taste * taste
        + w.session * session
        + w.context * context
        + w.season * season
        + w.event * event
        + w.curve * curve
        + w.universal * lib.universal[t]
        + w.filters * filters;
    if g == 2 && ctx.novelty > 0.0 {
        // «непохожесть»: правее — дальше от центра вкуса в пространстве признаков
        let mut d2 = 0.0;
        for i in 0..D {
            let d = x[i] - ctx.center[i];
            d2 += d * d;
        }
        s += 0.15 * ctx.novelty * (d2 / D as f32).sqrt();
    }
    Some((s, g))
}

fn keep_top(v: &mut Vec<Scored>, k: usize) {
    if v.len() > k {
        v.select_nth_unstable_by(k, |a, b| b.score.total_cmp(&a.score));
        v.truncate(k);
    }
    v.sort_unstable_by(|a, b| b.score.total_cmp(&a.score));
}

/// Этап 1, один поток: весь каталог → топ-K в каждой группе знакомости.
fn stage1(lib: &Library, ctx: &Ctx, w: &Weights, out: &mut [Vec<Scored>; 3]) {
    for g in out.iter_mut() {
        g.clear();
    }
    for t in 0..lib.len() {
        if let Some((s, g)) = score_track(lib, ctx, w, t) {
            out[g].push(Scored { score: s, idx: t as u32 });
        }
    }
    for g in out.iter_mut() {
        keep_top(g, TOP_K);
    }
}

/// Этап 1 на нескольких потоках (rayon в приложении; здесь — std::thread::scope).
fn stage1_par(lib: &Library, ctx: &Ctx, w: &Weights, threads: usize, out: &mut [Vec<Scored>; 3]) {
    let n = lib.len();
    let chunk = n.div_ceil(threads);
    let parts: Vec<[Vec<Scored>; 3]> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|k| {
                s.spawn(move || {
                    let mut local: [Vec<Scored>; 3] = Default::default();
                    for t in k * chunk..((k + 1) * chunk).min(n) {
                        if let Some((sc, g)) = score_track(lib, ctx, w, t) {
                            local[g].push(Scored { score: sc, idx: t as u32 });
                        }
                    }
                    for g in local.iter_mut() {
                        keep_top(g, TOP_K);
                    }
                    local
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or_default()).collect()
    });
    for g in out.iter_mut() {
        g.clear();
    }
    for part in parts {
        for (g, v) in part.into_iter().enumerate() {
            out[g].extend(v);
        }
    }
    for g in out.iter_mut() {
        keep_top(g, TOP_K);
    }
}

/// Состояние секвенсора (§26): последние исполнители, энергия/темп предыдущего, счётчики групп.
struct Seq {
    recent_artists: Vec<u32>,
    prev_energy: Option<f32>,
    prev_bpm: Option<f32>,
    group_counts: [u32; 3],
    slots: u32,
    queued: Vec<u32>,
}

impl Seq {
    fn new() -> Self {
        Seq {
            recent_artists: Vec::with_capacity(ARTIST_SPACING),
            prev_energy: None,
            prev_bpm: None,
            group_counts: [0; 3],
            slots: 0,
            queued: Vec::new(),
        }
    }
}

/// Этап 2: выбрать следующий трек. Группа — по наибольшему отставанию от квоты ползунка;
/// внутри группы — Томпсон по «отношению к треку» (бета), ограничения секвенсора,
/// бонус перехода автомикса, затем выборка из топ-20 по softmax с температурой.
fn pick_next(
    lib: &Library,
    ctx: &Ctx,
    cands: &[Vec<Scored>; 3],
    seq: &mut Seq,
    rng: &mut Rng,
) -> Option<u32> {
    let q = quotas(ctx.novelty);
    let mut order = [0usize, 1, 2];
    let deficit = |g: usize| q[g] * (seq.slots + 1) as f32 - seq.group_counts[g] as f32;
    order.sort_by(|&a, &b| deficit(b).total_cmp(&deficit(a)));
    let explore = 0.10 + 0.30 * ctx.novelty;
    let mut pool: Vec<(f32, u32)> = Vec::with_capacity(TOP_SAMPLE);
    for &g in &order {
        if q[g] <= 0.0 {
            continue;
        }
        pool.clear();
        for c in &cands[g] {
            let t = c.idx as usize;
            if seq.queued.contains(&c.idx) || seq.recent_artists.contains(&lib.artist[t]) {
                continue;
            }
            let x = &lib.feat[t];
            if let Some(pe) = seq.prev_energy {
                if (x[F_ENERGY] - pe).abs() > MAX_ENERGY_JUMP {
                    continue;
                }
            }
            let (a, b) = (lib.alpha[t], lib.beta[t]);
            let mut s = c.score + explore * (rng.beta(a, b) - a / (a + b));
            if let Some(pb) = seq.prev_bpm {
                let bpm = 60.0 + 140.0 * x[F_TEMPO];
                if ((bpm - pb) / pb).abs() <= 0.08 {
                    s += 0.02; // бонус, а не фильтр
                }
            }
            pool.push((s, c.idx));
            if pool.len() == TOP_SAMPLE {
                break;
            }
        }
        if pool.is_empty() {
            continue;
        }
        let temperature = 0.05;
        let max = pool.iter().map(|p| p.0).fold(f32::MIN, f32::max);
        let total: f32 = pool.iter().map(|p| ((p.0 - max) / temperature).exp()).sum();
        let mut r = rng.f32() * total;
        let mut chosen = pool[pool.len() - 1].1;
        for &(s, idx) in &pool {
            r -= ((s - max) / temperature).exp();
            if r <= 0.0 {
                chosen = idx;
                break;
            }
        }
        let t = chosen as usize;
        seq.queued.push(chosen);
        seq.recent_artists.push(lib.artist[t]);
        if seq.recent_artists.len() > ARTIST_SPACING - 1 {
            seq.recent_artists.remove(0);
        }
        seq.prev_energy = Some(lib.feat[t][F_ENERGY]);
        seq.prev_bpm = Some(60.0 + 140.0 * lib.feat[t][F_TEMPO]);
        seq.group_counts[g] += 1;
        seq.slots += 1;
        return Some(chosen);
    }
    None
}

struct Stats {
    p50: f64,
    p95: f64,
    max: f64,
}

fn measure<F: FnMut()>(iters: usize, mut f: F) -> Stats {
    for _ in 0..iters.min(20) {
        f(); // прогрев
    }
    let mut ms: Vec<f64> = (0..iters)
        .map(|_| {
            let t0 = Instant::now();
            f();
            t0.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    ms.sort_by(|a, b| a.total_cmp(b));
    Stats {
        p50: ms[ms.len() / 2],
        p95: ms[(ms.len() * 95 / 100).min(ms.len() - 1)],
        max: ms[ms.len() - 1],
    }
}

fn row(name: &str, s: &Stats) {
    println!("| {name} | {:.3} | {:.3} | {:.3} |", s.p50, s.p95, s.max);
}

fn base_ctx(lib: &Library, today: i32, rng: &mut Rng, model: &Taste) -> Ctx {
    let mut center = [0.0f32; D];
    for x in &lib.feat {
        for i in 0..D {
            center[i] += x[i];
        }
    }
    for v in center.iter_mut() {
        *v /= lib.len() as f32;
    }
    Ctx {
        today,
        season_w: calendar::season_weights(today, false, 21),
        w_taste: model.sample(rng, 0.3),
        b_taste: model.b,
        w_ctx: std::array::from_fn(|_| 0.2 * rng.normal()),
        w_session: [0.0; D],
        center,
        target_energy: None,
        curve_width: 0.35,
        event_active: false,
        max_vocal: 1.0,
        max_aggr: 1.0,
        soft_mood: None,
        novelty: 0.5,
        repeat_window_days: 1,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(50_000);
    let iters: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300);
    let today = calendar::days_from_civil(2026, 11, 22);
    let mut rng = Rng::new(0x5155_7700);

    let t0 = Instant::now();
    let mut lib = Library::synth(n, today, &mut rng);
    let synth_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let mut model = Taste::new();
    for _ in 0..2000 {
        let t = rng.below(n as u32) as usize;
        let y = if lib.liked[t] || lib.plays[t] > 4 { 1.0 } else { 0.0 };
        model.update(&lib.feat[t], y, 1.0);
    }
    let ctx = base_ctx(&lib, today, &mut rng, &model);
    let w = Weights::profile("my_wave");
    let cores = std::thread::available_parallelism().map(|v| v.get()).unwrap_or(4);

    println!("# S-17 — замеры (N = {n}, итераций = {iters}, ядер = {cores})\n");
    println!(
        "Синтез медиатеки: {synth_ms:.1} мс; матрица признаков: {:.1} МБ; все массивы движка: {:.1} МБ\n",
        (n * D * 4) as f64 / 1e6,
        lib.bytes() as f64 / 1e6
    );
    println!("| Операция | p50, мс | p95, мс | max, мс |\n|---|---|---|---|");

    let mut buf: [Vec<Scored>; 3] = Default::default();
    for g in buf.iter_mut() {
        g.reserve(n);
    }
    let s1 = measure(iters, || stage1(&lib, &ctx, &w, &mut buf));
    row("Этап 1: скоринг всей медиатеки, 1 поток («Моя волна»)", &s1);

    for profile in ["seasonal", "sleep", "for_all"] {
        let mut c2 = base_ctx(&lib, today, &mut rng, &model);
        if profile == "sleep" {
            c2.target_energy = Some(0.2);
            c2.max_aggr = 0.4;
            c2.novelty = 0.25;
        }
        let wp = Weights::profile(profile);
        let s = measure(iters, || stage1(&lib, &c2, &wp, &mut buf));
        row(&format!("Этап 1, профиль «{profile}»"), &s);
    }

    let threads = (cores / 2).clamp(2, 6);
    let sp = measure(iters, || stage1_par(&lib, &ctx, &w, threads, &mut buf));
    row(&format!("Этап 1, {threads} потока"), &sp);

    stage1(&lib, &ctx, &w, &mut buf);
    let cands = buf.clone();
    let s2 = measure(iters * 10, || {
        let mut seq = Seq::new();
        for _ in 0..5 {
            let _ = pick_next(&lib, &ctx, &cands, &mut seq, &mut rng);
        }
    });
    row("Этап 2: секвенсор, очередь из 5 треков", &s2);

    let full = measure(iters, || {
        let c = Ctx { w_taste: model.sample(&mut rng, 0.3), ..base_ctx_light(&ctx) };
        stage1(&lib, &c, &w, &mut buf);
        let mut seq = Seq::new();
        for _ in 0..5 {
            let _ = pick_next(&lib, &c, &buf, &mut seq, &mut rng);
        }
    });
    row("Полная пересборка очереди (Томпсон + этап 1 + этап 2)", &full);

    let x = lib.feat[0];
    let t0 = Instant::now();
    let updates = 1_000_000;
    for k in 0..updates {
        model.update(&x, (k % 2) as f32, 0.5);
    }
    let upd_ns = t0.elapsed().as_secs_f64() * 1e9 / updates as f64;
    println!("\nОбновление модели вкуса после события: {upd_ns:.0} нс на событие\n");

    // ---------- функциональные проверки ----------
    println!("## Функциональные проверки\n");
    for novelty in [0.0f32, 0.5, 1.0] {
        let saved_last_played = lib.last_played.clone();
        let mut c = base_ctx(&lib, today, &mut rng, &model);
        c.novelty = novelty;
        let mut seq = Seq::new();
        let mut never_hits = 0;
        let mut spacing_violations = 0;
        let mut history: Vec<u32> = Vec::new();
        for _ in 0..40 {
            stage1(&lib, &c, &w, &mut buf);
            for _ in 0..5 {
                if let Some(t) = pick_next(&lib, &c, &buf, &mut seq, &mut rng) {
                    let t = t as usize;
                    if lib.never[t] {
                        never_hits += 1;
                    }
                    let a = lib.artist[t];
                    if history.iter().rev().take(ARTIST_SPACING - 1).any(|&p| lib.artist[p as usize] == a) {
                        spacing_violations += 1;
                    }
                    history.push(t as u32);
                    lib.last_played[t] = today; // сыграно → окно повтора
                }
            }
            seq.queued.clear();
        }
        let total = seq.slots.max(1) as f32;
        let q = quotas(novelty);
        println!(
            "- Ползунок {:>3}: доли групп [знакомое, забытое, новое] = [{:.2}, {:.2}, {:.2}] (цель [{:.2}, {:.2}, {:.2}]); «никогда» в очереди: {never_hits}; нарушений разноса исполнителей: {spacing_violations}",
            (novelty * 100.0) as u32,
            seq.group_counts[0] as f32 / total,
            seq.group_counts[1] as f32 / total,
            seq.group_counts[2] as f32 / total,
            q[0],
            q[1],
            q[2]
        );
        lib.last_played = saved_last_played;
    }

    // засыпательная волна: целевая энергия падает от 0,6 до 0,1 за 10 пересборок
    let mut c = base_ctx(&lib, today, &mut rng, &model);
    c.novelty = 0.2;
    c.max_aggr = 0.4;
    let ws = Weights::profile("sleep");
    let mut energies = Vec::new();
    let mut seq = Seq::new();
    for step in 0..10 {
        c.target_energy = Some(0.6 - 0.05 * step as f32);
        seq.prev_energy = None; // кривая сама ведёт энергию вниз
        stage1(&lib, &c, &ws, &mut buf);
        let mut sum = 0.0;
        let mut k = 0;
        for _ in 0..3 {
            if let Some(t) = pick_next(&lib, &c, &buf, &mut seq, &mut rng) {
                sum += lib.feat[t as usize][F_ENERGY];
                k += 1;
            }
        }
        energies.push(sum / k.max(1) as f32);
    }
    println!(
        "- Сон: средняя энергия по шагам кривой = {:?}",
        energies.iter().map(|e| (e * 100.0).round() / 100.0).collect::<Vec<_>>()
    );
}

/// Копия контекста без пересчёта центра вкуса (центр кэшируется до изменения медиатеки).
fn base_ctx_light(c: &Ctx) -> Ctx {
    Ctx {
        today: c.today,
        season_w: c.season_w,
        w_taste: c.w_taste,
        b_taste: c.b_taste,
        w_ctx: c.w_ctx,
        w_session: c.w_session,
        center: c.center,
        target_energy: c.target_energy,
        curve_width: c.curve_width,
        event_active: c.event_active,
        max_vocal: c.max_vocal,
        max_aggr: c.max_aggr,
        soft_mood: c.soft_mood,
        novelty: c.novelty,
        repeat_window_days: c.repeat_window_days,
    }
}
