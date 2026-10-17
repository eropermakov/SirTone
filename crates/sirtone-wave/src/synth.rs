//! Synthetic library for tests and the engine benchmark (T3.24; the S-17 generator): realistic
//! feature correlations (energy ↔ tempo/danceability, valence ↔ major), a skewed artist
//! distribution, history (≈ 40 % untouched), likes and «никогда» (0.2 %). Deterministic by seed.

use sirtone_core::wave::{FEATURES as D, WaveTrack};

use crate::rng::Rng;

const DAY_MS: i64 = 24 * 3600 * 1000;
const GENRES: [&str; 6] = ["Pop", "Rock", "Electronic", "Hip-Hop", "Jazz", "Folk"];

/// `n` tracks around `now_ms`; analysed features for a share `analysed` of them.
pub fn library(n: usize, now_ms: i64, analysed: f32, seed: u64) -> Vec<WaveTrack> {
    let mut rng = Rng::new(seed);
    let n_artists = (n / 10).max(1) as u32;
    (0..n)
        .map(|i| {
            let energy = rng.f32();
            let valence = rng.f32();
            let mut x = [0f32; D];
            x[0] = energy;
            x[1] = valence;
            x[2] = (0.7 * energy + 0.3 * rng.f32()).min(1.0);
            x[3] = (0.5 * energy + 0.5 * rng.f32()).min(1.0);
            x[4] = (0.6 * energy + 0.4 * rng.f32()).min(1.0);
            let key = rng.below(12) as f32 * std::f32::consts::TAU / 12.0;
            x[5] = 0.5 + 0.5 * key.sin();
            x[6] = 0.5 + 0.5 * key.cos();
            x[7] = f32::from(u8::from(rng.f32() < 0.4 + 0.4 * valence));
            for v in x.iter_mut().take(26).skip(8) {
                *v = rng.f32();
            }
            let genre = rng.below(6) as usize;
            x[26 + genre] = 1.0;
            let r = rng.f32();
            let (plays, liked) = if r < 0.40 {
                (0, false)
            } else if r < 0.80 {
                (1 + rng.below(4), false)
            } else {
                (5 + rng.below(40), rng.f32() < 0.3)
            };
            let artist = ((rng.f32() * rng.f32() * n_artists as f32) as u32).min(n_artists - 1);
            WaveTrack {
                id: i as i64 + 1,
                artist_id: i64::from(artist) + 1,
                duration_ms: 120_000 + u64::from(rng.below(240_000)),
                year: Some(1960 + rng.below(66) as i32),
                genre: Some(GENRES[genre].to_string()),
                added_ms: now_ms - i64::from(rng.below(5 * 365)) * DAY_MS,
                play_count: plays,
                skip_count: rng.below(3),
                last_played_ms: (plays > 0)
                    .then(|| now_ms - DAY_MS - i64::from(rng.below(2 * 365)) * DAY_MS),
                completion: rng.f32(),
                liked,
                never: rng.f32() < 0.002,
                features: (rng.f32() < analysed).then_some((x, 1.0)),
            }
        })
        .collect()
}
