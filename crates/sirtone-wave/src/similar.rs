//! Playlist continuation (T3.14, FR-PLS-003, docs/04 §11; `01b` F-14 — Roon Radio, Plexamp,
//! Spotify Smart Shuffle): «похожие только из медиатеки», ordered by closeness to the playlist,
//! stopping when nothing close enough is left.
//!
//! The sound vector is in blocks — the tagger embedding (B2), tempo on a log2 circle (octave
//! errors of BPM land on the same point), key on the circle of fifths (B1) and
//! valence/arousal/energy (B3) — each centred and scaled; a pair is compared on the blocks both
//! tracks have, so a library half-way through analysis still compares sensibly. One greedy pass
//! (closeness + continuity from the previous pick + novelty, MMR spreading, no two picks of one
//! artist close together) gives the same order on the playlist page and in the queue: the
//! queue only continues the sequence. No randomness.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use sirtone_core::wave::SimTrack;

use crate::engine::camelot_parts;

const EMB: usize = 64;
/// Candidates kept for the greedy pass (the closest ones; the rest are never reached).
const POOL: usize = 400;
/// Picks the MMR redundancy looks back at.
const MMR_WINDOW: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum SimError {
    #[error("continuation config: {0}")]
    Json(#[from] serde_json::Error),
    #[error("continuation config: schema")]
    Schema,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockWeights {
    pub embedding: f32,
    pub tempo: f32,
    pub key: f32,
    pub vibe: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScoreWeights {
    pub closeness: f32,
    pub continuity: f32,
    pub novelty: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Theta {
    pub k: f32,
    pub lo: f32,
    pub hi: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SimConfig {
    schema: String,
    pub weights: BlockWeights,
    pub score: ScoreWeights,
    pub mmr_lambda: f32,
    pub artist_penalty: f32,
    pub artist_window: usize,
    pub theta: Theta,
    pub max_picks: usize,
}

impl SimConfig {
    pub fn builtin() -> Result<Self, SimError> {
        Self::from_json(include_str!("../../../assets/wave/continuation.json"))
    }

    pub fn from_json(s: &str) -> Result<Self, SimError> {
        let c: Self = serde_json::from_str(s)?;
        if c.schema != "sirtone.continuation/1" {
            return Err(SimError::Schema);
        }
        Ok(c)
    }
}

/// A point in the blocked space: a track or the centre of a playlist.
#[derive(Debug, Clone, Default)]
struct Point {
    emb: Option<[f32; EMB]>,
    tempo: Option<[f32; 2]>,
    key: Option<[f32; 2]>,
    vae: Option<[f32; 3]>,
}

/// One pick of the continuation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pick {
    pub id: i64,
    /// Closeness to the playlist (cosine), the order «по близости».
    pub closeness: f32,
}

/// The library prepared for continuation (rebuilt when the library or the analysis changes).
pub struct Similar {
    cfg: SimConfig,
    ids: Vec<i64>,
    index: HashMap<i64, usize>,
    artist: Vec<i64>,
    plays: Vec<u32>,
    /// Centred, unit-length embeddings as i8 (×127); `has_emb` says which rows are real.
    emb: Vec<i8>,
    has_emb: Vec<bool>,
    tempo: Vec<Option<[f32; 2]>>,
    key: Vec<Option<[f32; 2]>>,
    vae: Vec<Option<[f32; 3]>>,
}

fn dequantise(blob: &[u8]) -> Option<[f32; EMB]> {
    if blob.len() != 4 + EMB {
        return None;
    }
    let scale = f32::from_le_bytes([blob[0], blob[1], blob[2], blob[3]]);
    let mut v = [0f32; EMB];
    for (o, b) in v.iter_mut().zip(&blob[4..]) {
        *o = f32::from(*b as i8) * scale;
    }
    Some(v)
}

fn circle(turns: f32) -> [f32; 2] {
    let a = turns * std::f32::consts::TAU;
    [a.sin(), a.cos()]
}

fn dot<const N: usize>(a: &[f32; N], b: &[f32; N]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

impl Similar {
    pub fn build(tracks: &[SimTrack], cfg: SimConfig) -> Self {
        let n = tracks.len();
        let raw: Vec<Option<[f32; EMB]>> = tracks
            .iter()
            .map(|t| t.embedding.as_deref().and_then(dequantise))
            .collect();
        // Centre by the library mean: the pooled activations share a large common part that
        // would make every pair look alike.
        let mut mean = [0f32; EMB];
        let mut k = 0f32;
        for v in raw.iter().flatten() {
            for (m, x) in mean.iter_mut().zip(v) {
                *m += x;
            }
            k += 1.0;
        }
        if k > 0.0 {
            for m in &mut mean {
                *m /= k;
            }
        }
        let mut emb = vec![0i8; n * EMB];
        let mut has_emb = vec![false; n];
        for (i, v) in raw.iter().enumerate() {
            let Some(v) = v else { continue };
            let c: Vec<f32> = v.iter().zip(&mean).map(|(x, m)| x - m).collect();
            let norm = c.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm < 1e-9 {
                continue;
            }
            for (o, x) in emb[i * EMB..(i + 1) * EMB].iter_mut().zip(&c) {
                *o = (x / norm * 127.0).round().clamp(-127.0, 127.0) as i8;
            }
            has_emb[i] = true;
        }
        // Valence/arousal/energy: centred and divided by the library spread (per dimension),
        // so the block's length is about 1 like the others.
        let vae_raw: Vec<Option<[f32; 3]>> = tracks.iter().map(|t| t.vae).collect();
        let (mut mu, mut m2, mut cnt) = ([0f32; 3], [0f32; 3], 0f32);
        for v in vae_raw.iter().flatten() {
            cnt += 1.0;
            for d in 0..3 {
                let delta = v[d] - mu[d];
                mu[d] += delta / cnt;
                m2[d] += delta * (v[d] - mu[d]);
            }
        }
        let sd = m2.map(|s| (s / cnt.max(1.0)).sqrt().max(0.05));
        let vae = vae_raw
            .iter()
            .map(|v| {
                v.map(|v| {
                    let s = 3f32.sqrt();
                    [
                        (v[0] - mu[0]) / sd[0] / s,
                        (v[1] - mu[1]) / sd[1] / s,
                        (v[2] - mu[2]) / sd[2] / s,
                    ]
                })
            })
            .collect();
        Self {
            ids: tracks.iter().map(|t| t.id).collect(),
            index: tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect(),
            artist: tracks.iter().map(|t| t.artist_id).collect(),
            plays: tracks.iter().map(|t| t.play_count).collect(),
            emb,
            has_emb,
            tempo: tracks
                .iter()
                .map(|t| t.bpm.filter(|b| *b > 0.0).map(|b| circle(b.log2())))
                .collect(),
            key: tracks
                .iter()
                .map(|t| {
                    t.key_camelot
                        .as_deref()
                        .and_then(camelot_parts)
                        .map(|(n, _)| circle((n - 1) as f32 / 12.0))
                })
                .collect(),
            vae,
            cfg,
        }
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    fn point(&self, i: usize) -> Point {
        Point {
            emb: self.has_emb[i].then(|| {
                let mut v = [0f32; EMB];
                for (o, q) in v.iter_mut().zip(&self.emb[i * EMB..(i + 1) * EMB]) {
                    *o = f32::from(*q) / 127.0;
                }
                v
            }),
            tempo: self.tempo[i],
            key: self.key[i],
            vae: self.vae[i],
        }
    }

    /// Cosine over the blocks both have (the weights as in a concatenated vector); `None`
    /// when they share none.
    fn cos(&self, a: &Point, b: usize) -> Option<f32> {
        let w = &self.cfg.weights;
        let (mut num, mut na, mut nb) = (0f32, 0f32, 0f32);
        let mut shared = false;
        if let Some(e) = &a.emb
            && self.has_emb[b]
        {
            let q = &self.emb[b * EMB..(b + 1) * EMB];
            let (mut d, mut n2) = (0f32, 0f32);
            for (x, y) in e.iter().zip(q) {
                let y = f32::from(*y) / 127.0;
                d += x * y;
                n2 += y * y;
            }
            let w2 = w.embedding * w.embedding;
            num += w2 * d;
            na += w2 * dot(e, e);
            nb += w2 * n2;
            shared = true;
        }
        let mut block = |x: Option<&[f32]>, y: Option<&[f32]>, wt: f32| {
            if let (Some(x), Some(y)) = (x, y) {
                let w2 = wt * wt;
                num += w2 * x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>();
                na += w2 * x.iter().map(|p| p * p).sum::<f32>();
                nb += w2 * y.iter().map(|q| q * q).sum::<f32>();
                shared = true;
            }
        };
        block(
            a.tempo.as_ref().map(|v| &v[..]),
            self.tempo[b].as_ref().map(|v| &v[..]),
            w.tempo,
        );
        block(
            a.key.as_ref().map(|v| &v[..]),
            self.key[b].as_ref().map(|v| &v[..]),
            w.key,
        );
        block(
            a.vae.as_ref().map(|v| &v[..]),
            self.vae[b].as_ref().map(|v| &v[..]),
            w.vibe,
        );
        (shared && na > 1e-12 && nb > 1e-12).then(|| num / (na * nb).sqrt())
    }

    /// Closeness of two library tracks (`None` — not comparable yet).
    pub fn pair(&self, a: i64, b: i64) -> Option<f32> {
        let (a, b) = (*self.index.get(&a)?, *self.index.get(&b)?);
        self.cos(&self.point(a), b)
    }

    /// The centre of the playlist; later tracks weigh a little more (the playlist «drifts»).
    fn centroid(&self, seeds: &[usize]) -> Point {
        let n = seeds.len().max(1) as f32;
        let mut emb = ([0f32; EMB], 0f32);
        let mut tempo = ([0f32; 2], 0f32);
        let mut key = ([0f32; 2], 0f32);
        let mut vae = ([0f32; 3], 0f32);
        fn add<const N: usize>(acc: &mut ([f32; N], f32), v: &[f32; N], w: f32) {
            for (a, x) in acc.0.iter_mut().zip(v) {
                *a += w * x;
            }
            acc.1 += w;
        }
        for (pos, &i) in seeds.iter().enumerate() {
            let w = 0.5 + 0.5 * (pos + 1) as f32 / n;
            let p = self.point(i);
            if let Some(v) = &p.emb {
                add(&mut emb, v, w);
            }
            if let Some(v) = &p.tempo {
                add(&mut tempo, v, w);
            }
            if let Some(v) = &p.key {
                add(&mut key, v, w);
            }
            if let Some(v) = &p.vae {
                add(&mut vae, v, w);
            }
        }
        fn mean<const N: usize>(acc: ([f32; N], f32)) -> Option<[f32; N]> {
            (acc.1 > 0.0).then(|| acc.0.map(|x| x / acc.1))
        }
        Point {
            emb: mean(emb),
            tempo: mean(tempo),
            key: mean(key),
            vae: mean(vae),
        }
    }

    /// Closeness of a track to the centre of a playlist (calibration, «Почему»).
    pub fn closeness(&self, seeds: &[i64], id: i64) -> Option<f32> {
        let idx: Vec<usize> = seeds
            .iter()
            .filter_map(|id| self.index.get(id).copied())
            .collect();
        let i = *self.index.get(&id)?;
        (!idx.is_empty()).then(|| self.cos(&self.centroid(&idx), i))?
    }

    /// The stop threshold θ = clamp(μ − k·σ, lo, hi) over the pairwise closeness inside the
    /// playlist (docs/04 §11); a single comparable pair or none — the lower bound.
    pub fn theta(&self, seeds: &[i64]) -> f32 {
        let idx: Vec<usize> = seeds
            .iter()
            .filter_map(|id| self.index.get(id).copied())
            .collect();
        let mut v = Vec::new();
        for (a, &i) in idx.iter().enumerate() {
            let p = self.point(i);
            for &j in &idx[a + 1..] {
                if let Some(c) = self.cos(&p, j) {
                    v.push(c);
                }
            }
        }
        let t = &self.cfg.theta;
        if v.len() < 2 {
            return t.lo;
        }
        let mu = v.iter().sum::<f32>() / v.len() as f32;
        let sd = (v.iter().map(|x| (x - mu).powi(2)).sum::<f32>() / v.len() as f32).sqrt();
        (mu - t.k * sd).clamp(t.lo, t.hi)
    }

    /// The continuation of a playlist (`seeds`, in order): up to `max_picks` tracks that are
    /// not seeds and not `exclude`d («−», «никогда», recently played), closest first with
    /// spreading. `already` — picks made before (queued or played in this session): the
    /// sequence goes on from them, so the queue and the page agree. Empty — nothing is close
    /// enough: playback stops after the playlist (FR-PLS-003).
    pub fn continuation(
        &self,
        seeds: &[i64],
        exclude: &HashSet<i64>,
        already: &[i64],
        want: usize,
    ) -> Vec<Pick> {
        let seed_idx: Vec<usize> = seeds
            .iter()
            .filter_map(|id| self.index.get(id).copied())
            .collect();
        if seed_idx.is_empty() {
            return Vec::new();
        }
        let theta = self.theta(seeds);
        let c = self.centroid(&seed_idx);
        let seed_set: HashSet<i64> = seeds.iter().copied().collect();
        let already_set: HashSet<i64> = already.iter().copied().collect();
        let mut pool: Vec<(usize, f32)> = (0..self.ids.len())
            .filter(|&i| {
                let id = self.ids[i];
                !seed_set.contains(&id) && !exclude.contains(&id) && !already_set.contains(&id)
            })
            .filter_map(|i| self.cos(&c, i).filter(|v| *v >= theta).map(|v| (i, v)))
            .collect();
        pool.sort_by(|a, b| b.1.total_cmp(&a.1).then(self.ids[a.0].cmp(&self.ids[b.0])));
        pool.truncate(POOL);
        // The sequence so far: the playlist's last track, then the earlier picks.
        let mut history: Vec<usize> = seed_idx.last().copied().into_iter().collect();
        history.extend(already.iter().filter_map(|id| self.index.get(id).copied()));
        let budget = self.cfg.max_picks.saturating_sub(already.len());
        let s = &self.cfg.score;
        let mut out = Vec::new();
        while out.len() < want.min(budget) && !pool.is_empty() {
            let prev = history.last().map(|&i| self.point(i));
            let recent: Vec<Point> = history
                .iter()
                .rev()
                .take(MMR_WINDOW)
                .map(|&i| self.point(i))
                .collect();
            let artists: Vec<i64> = history
                .iter()
                .rev()
                .take(self.cfg.artist_window)
                .map(|&i| self.artist[i])
                .collect();
            let mut best: Option<(usize, f32)> = None;
            for (k, &(i, close)) in pool.iter().enumerate() {
                let cont = prev.as_ref().and_then(|p| self.cos(p, i)).unwrap_or(0.0);
                let novelty = 1.0 / (1.0 + self.plays[i] as f32);
                let score = s.closeness * close + s.continuity * cont + s.novelty * novelty;
                let redundancy = recent
                    .iter()
                    .filter_map(|p| self.cos(p, i))
                    .fold(0f32, f32::max);
                let mut mmr =
                    self.cfg.mmr_lambda * score - (1.0 - self.cfg.mmr_lambda) * redundancy;
                // The same artist: the full penalty right after, less further back.
                if let Some(pos) = artists.iter().position(|a| *a == self.artist[i]) {
                    mmr -= self.cfg.artist_penalty
                        * (1.0 - pos as f32 / self.cfg.artist_window.max(1) as f32);
                }
                if best.is_none_or(|(_, b)| mmr > b) {
                    best = Some((k, mmr));
                }
            }
            let Some((k, _)) = best else { break };
            let (i, close) = pool.remove(k);
            history.push(i);
            out.push(Pick {
                id: self.ids[i],
                closeness: close,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A track whose embedding points along `dir` (plus the shared component every real
    /// embedding has) with `noise`.
    fn track(id: i64, artist: i64, dir: usize, noise: u64) -> SimTrack {
        let mut v = [0f32; EMB];
        let mut s = noise.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        for (i, x) in v.iter_mut().enumerate() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            *x = 3.0 + 0.15 * ((s % 1000) as f32 / 1000.0 - 0.5) + if i == dir { 1.0 } else { 0.0 };
        }
        let scale = 4.5f32 / 127.0;
        let mut blob = scale.to_le_bytes().to_vec();
        blob.extend(v.iter().map(|x| (x / scale).round() as i8 as u8));
        SimTrack {
            id,
            artist_id: artist,
            play_count: 0,
            bpm: Some(120.0),
            key_camelot: Some("8B".into()),
            vae: Some([0.5, 0.5, 0.5]),
            embedding: Some(blob),
        }
    }

    fn library() -> Vec<SimTrack> {
        let mut t = Vec::new();
        // Cluster A (direction 0): ids 1–8, two artists; cluster B (direction 1): 11–18;
        // cluster C (direction 2): 21–28.
        for (base, dir) in [(1, 0), (11, 1), (21, 2)] {
            for k in 0..8 {
                t.push(track(base + k, base + k % 2, dir, (base + k) as u64));
            }
        }
        t
    }

    fn sim() -> Similar {
        Similar::build(&library(), SimConfig::builtin().expect("config"))
    }

    #[test]
    fn closest_first_then_stop() {
        let s = sim();
        // Same cluster ≫ another cluster, despite the shared component.
        let same = s.pair(1, 2).expect("comparable");
        let other = s.pair(1, 11).expect("comparable");
        assert!(same > 0.6 && other < 0.3, "{same} {other}");
        // A playlist of three A tracks continues with the rest of A only, then stops.
        let picks = s.continuation(&[1, 2, 3], &HashSet::new(), &[], 50);
        let ids: Vec<i64> = picks.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), 5, "{picks:?}");
        assert!(ids.iter().all(|id| (4..=8).contains(id)), "{ids:?}");
        // «Ближе — раньше» within spreading: the first pick is one of the two closest.
        let mut c: Vec<f32> = picks.iter().map(|p| p.closeness).collect();
        c.sort_by(|a, b| b.total_cmp(a));
        assert!(picks[0].closeness >= c[1], "{picks:?}");
    }

    #[test]
    fn queue_continues_the_page_sequence_and_excludes() {
        let s = sim();
        let page = s.continuation(&[11, 12, 13], &HashSet::new(), &[], 50);
        let head: Vec<i64> = page.iter().take(2).map(|p| p.id).collect();
        let rest = s.continuation(&[11, 12, 13], &HashSet::new(), &head, 50);
        assert_eq!(
            rest.iter().map(|p| p.id).collect::<Vec<_>>(),
            page[2..].iter().map(|p| p.id).collect::<Vec<_>>()
        );
        // «−» on a track: it never comes back.
        let minus: HashSet<i64> = [page[0].id].into();
        let after = s.continuation(&[11, 12, 13], &minus, &[], 50);
        assert!(after.iter().all(|p| p.id != page[0].id));
        assert_eq!(after.len(), page.len() - 1);
    }

    #[test]
    fn artists_are_spread() {
        let s = sim();
        let picks = s.continuation(&[21], &HashSet::new(), &[], 50);
        // Two artists alternate in cluster C: no artist twice in a row.
        for w in picks.windows(2) {
            assert_ne!(s.artist[s.index[&w[0].id]], s.artist[s.index[&w[1].id]]);
        }
    }

    #[test]
    fn unanalysed_tracks_compare_on_what_they_have() {
        let mut lib = library();
        lib.push(SimTrack {
            id: 99,
            artist_id: 99,
            play_count: 0,
            bpm: Some(240.0), // an octave of 120: the same point on the tempo circle
            key_camelot: Some("8B".into()),
            vae: None,
            embedding: None,
        });
        lib.push(SimTrack {
            id: 100,
            artist_id: 100,
            play_count: 0,
            bpm: None,
            key_camelot: None,
            vae: None,
            embedding: None,
        });
        let s = Similar::build(&lib, SimConfig::builtin().expect("config"));
        assert!(s.pair(1, 99).expect("tempo and key") > 0.99);
        assert_eq!(s.pair(1, 100), None);
    }
}
