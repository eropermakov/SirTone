//! Synthetic library for tests and the engine benchmark (T3.24; the S-17 generator): realistic
//! feature correlations (energy ↔ tempo/danceability, valence ↔ major), a skewed artist
//! distribution, history (≈ 40 % untouched), likes and «никогда» (0.2 %). Deterministic by seed.

use sirtone_core::wave::{
    ContextSnapshot, Daypart, EndReason, FEATURES as D, FeedbackRow, PlayEvent, PlaySource,
    WaveTrack,
};

use crate::learn::History;
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
                lufs: None,
                lra: None,
                soft_intro: None,
                soft_outro: None,
                title: Some(if i % 40 == 0 {
                    format!("Winter Song {i}")
                } else {
                    format!("Track number {i}")
                }),
                album: Some(format!("Album {}", i / 12)),
                artist_name: Some(format!("Artist {artist}")),
                season_tags: 0,
                plays_weeks: {
                    let mut w = [0u8; 13];
                    w[0] = plays.min(255) as u8;
                    w
                },
            }
        })
        .collect()
}

/// A journal consistent with the library: plays per `play_count` (finished), skips per
/// `skip_count`, likes and «никогда» as signals.
pub fn history(tracks: &[WaveTrack], now_ms: i64) -> History {
    let mut h = History::default();
    for t in tracks {
        let ctx = |ts: i64| ContextSnapshot {
            now_utc_ms: ts,
            local_minute: 20 * 60,
            weekday: 2,
            daypart: Daypart::Evening,
            season_w: [0.0, 0.0, 0.0, 1.0],
            weather: None,
        };
        let last = t.last_played_ms.unwrap_or(now_ms - 400 * DAY_MS);
        for k in 0..t.play_count {
            let ts = last - i64::from(k) * 3 * DAY_MS;
            h.events.push(PlayEvent {
                ts,
                track_id: t.id,
                played_ms: t.duration_ms,
                duration_ms: t.duration_ms,
                end_reason: EndReason::Finished,
                source: PlaySource::Wave,
                profile: None,
                context: ctx(ts),
                event_id: None,
                novelty: None,
                category: None,
            });
        }
        for k in 0..t.skip_count {
            let ts = last - i64::from(k) * 5 * DAY_MS - 1000;
            h.events.push(PlayEvent {
                ts,
                track_id: t.id,
                played_ms: 10_000,
                duration_ms: t.duration_ms,
                end_reason: EndReason::Skip,
                source: PlaySource::Wave,
                profile: None,
                context: ctx(ts),
                event_id: None,
                novelty: None,
                category: None,
            });
        }
        for (on, kind) in [(t.liked, "like"), (t.never, "never")] {
            if on {
                h.feedback.push(FeedbackRow {
                    ts: last,
                    track_id: Some(t.id),
                    artist_id: None,
                    kind: kind.into(),
                    ctx_key: None,
                });
            }
        }
    }
    h.events.sort_by_key(|e| e.ts);
    h.feedback.sort_by_key(|f| f.ts);
    h
}
