//! Learning (T3.28, docs/14 §8): the models are a deterministic replay of the journal — every
//! play with its context and every signal — so nothing has to be stored, reset is «learn from
//! now on» and switching learning off freezes the models at a moment (the caller passes only
//! the events it may learn from). Signals → updates per §8.1: beta per track (half-life 90
//! days), taste (Bayesian logistic regression), context on levels daypart → × season →
//! × weather with fallback λ = n/(n + 20), the session vector (half-life 10 min).

use std::collections::HashMap;

use sirtone_core::wave::{
    ContextSnapshot, EndReason, FEATURES as D, FeedbackRow, PlayEvent, PlaySource,
};

use crate::engine::{Catalog, Config, Taste};

const DAY_MS: f32 = 24.0 * 3600.0 * 1000.0;
const MIN_MS: f32 = 60_000.0;

/// What the models learn from (already cut by the reset and freeze marks).
#[derive(Debug, Clone, Default)]
pub struct History {
    pub events: Vec<PlayEvent>,
    pub feedback: Vec<FeedbackRow>,
}

/// The context levels of a moment, narrow to wide is reversed: [daypart, ×season, ×weather].
pub fn levels(daypart: u8, season: usize, weather: Option<u8>) -> [String; 3] {
    let w = weather.map_or_else(|| "x".to_string(), |w| w.to_string());
    [
        format!("d{daypart}"),
        format!("d{daypart}/s{season}"),
        format!("d{daypart}/s{season}/w{w}"),
    ]
}

fn dominant_season(w: &[f32; 4]) -> usize {
    w.iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(i, _)| i)
}

pub fn ctx_levels(ctx: &ContextSnapshot) -> [String; 3] {
    levels(
        ctx.daypart as u8,
        dominant_season(&ctx.season_w),
        ctx.weather.map(|w| w as u8),
    )
}

/// Levels of a stored bucket key `d{d}/s{s}/w{w}` (D-77) or a legacy `daypart:N`.
fn key_levels(key: &str) -> Option<Vec<String>> {
    if let Some(d) = key.strip_prefix("daypart:") {
        return Some(vec![format!("d{d}")]);
    }
    let parts: Vec<&str> = key.split('/').collect();
    let d = parts.first()?;
    let mut out = vec![(*d).to_string()];
    if let Some(s) = parts.get(1) {
        out.push(format!("{d}/{s}"));
        if let Some(w) = parts.get(2) {
            out.push(format!("{d}/{s}/{w}"));
        }
    }
    Some(out)
}

pub struct Models {
    pub alpha: Vec<f32>,
    pub beta: Vec<f32>,
    /// The guests' model (`learning_policy: guests`, «Для всех»): beta per track.
    pub guest_alpha: Vec<f32>,
    pub guest_beta: Vec<f32>,
    pub taste: Taste,
    /// Context level → (model, number of signals).
    pub context: HashMap<String, (Taste, u32)>,
    pub session: [f32; D],
    /// The last skips of the session: (catalog index, decay) — the repel of D-92.
    pub skips: Vec<(usize, f32)>,
}

/// Replays the history into fresh models (§8.1 table → §8.2 updates).
pub fn replay(cfg: &Config, cat: &Catalog, h: &History, now_ms: i64) -> Models {
    let n = cat.len();
    let e = &cfg.engine;
    let s = &e.signals;
    let mut m = Models {
        alpha: vec![1.0; n],
        beta: vec![1.0; n],
        guest_alpha: vec![1.0; n],
        guest_beta: vec![1.0; n],
        taste: Taste::default(),
        context: HashMap::new(),
        session: [0.0; D],
        skips: Vec::new(),
    };
    let half = e.beta_half_life_days;
    let decay = |ts: i64| 2f32.powf(-((now_ms - ts).max(0) as f32 / DAY_MS) / half);
    let center = cat.center();
    let sess = &e.session;
    let sess_decay = |ts: i64| {
        let age = (now_ms - ts).max(0) as f32 / MIN_MS;
        (age <= sess.window_min).then(|| 2f32.powf(-age / sess.half_life_min))
    };
    // One update of every model a signal reaches.
    let apply = |m: &mut Models, i: usize, y: f32, w: f32, ts: i64, levels: Option<&[String]>| {
        let k = w * decay(ts);
        if y > 0.5 {
            m.alpha[i] += k;
        } else {
            m.beta[i] += k;
        }
        let x = cat.feat[i];
        m.taste.update(&x, y, w);
        if let Some(levels) = levels {
            for key in levels {
                let entry = m.context.entry(key.clone()).or_default();
                entry.0.update(&x, y, w);
                entry.1 += 1;
            }
        }
    };
    // Plays and signals in a guests' profile («Для всех») teach only the guests' model; the
    // owner's models stay exactly as they were (§8, AT-20).
    let guests = |p: &Option<String>| {
        p.as_deref()
            .is_some_and(|p| cfg.guest_profiles.iter().any(|g| g == p))
    };
    let guest = |m: &mut Models, i: usize, y: f32, w: f32, ts: i64| {
        let k = w * decay(ts);
        if y > 0.5 {
            m.guest_alpha[i] += k;
        } else {
            m.guest_beta[i] += k;
        }
    };
    // The sleep wave (`context_only`, §14, FR-SLEEP-008): falling asleep to a track says
    // nothing about taste; a skip teaches only the «sleep» context.
    let context_only = |p: &Option<String>| {
        p.as_deref()
            .is_some_and(|p| cfg.context_only_profiles.iter().any(|g| g == p))
    };
    for ev in &h.events {
        let Some(i) = cat.idx(ev.track_id) else {
            continue;
        };
        if context_only(&ev.profile) {
            if ev.end_reason == EndReason::Skip {
                let w = if ev.played_ms < 30_000 {
                    s.skip_early
                } else {
                    s.skip_mid
                };
                let entry = m
                    .context
                    .entry(crate::engine::SLEEP_CONTEXT.to_string())
                    .or_default();
                entry.0.update(&cat.feat[i], 0.0, w * decay(ev.ts));
                entry.1 += 1;
            }
            continue;
        }
        if guests(&ev.profile) {
            match ev.end_reason {
                EndReason::Finished | EndReason::MixedOut => {
                    guest(&mut m, i, 1.0, s.finished, ev.ts)
                }
                EndReason::Skip => {
                    let w = if ev.played_ms < 30_000 {
                        s.skip_early
                    } else if ev.played_ms < ev.duration_ms / 2 {
                        s.skip_mid
                    } else {
                        s.skip_late
                    };
                    guest(&mut m, i, 0.0, w, ev.ts);
                }
                _ => {}
            }
            continue;
        }
        let lv = ctx_levels(&ev.context);
        let new_for_you = ev.category == Some(2);
        match ev.end_reason {
            EndReason::Finished | EndReason::MixedOut => {
                let w = if new_for_you {
                    s.finished_new
                } else {
                    s.finished
                };
                apply(&mut m, i, 1.0, w, ev.ts, Some(&lv));
            }
            EndReason::Skip => {
                let w = if ev.played_ms < 30_000 {
                    s.skip_early
                } else if ev.played_ms < ev.duration_ms / 2 {
                    s.skip_mid
                } else {
                    s.skip_late
                };
                apply(&mut m, i, 0.0, w, ev.ts, Some(&lv));
                if let Some(k) = sess_decay(ev.ts) {
                    // Only an analysed track says what «такое» is: unanalysed ones all carry
                    // the library's means (D-92).
                    if cat.conf[i] > 0.0 {
                        m.skips.push((i, k));
                    }
                    for ((sv, xv), cv) in m
                        .session
                        .iter_mut()
                        .zip(cat.feat[i].iter())
                        .zip(center.iter())
                    {
                        *sv -= sess.skip_step * k * (xv - cv);
                    }
                }
            }
            // Pause, sleep, quit, errors — not a signal (§8.1).
            _ => {}
        }
        // Chosen by hand from the collection, search, an album, a playlist (§8.1).
        if matches!(
            ev.source,
            PlaySource::Collection
                | PlaySource::Search
                | PlaySource::Album
                | PlaySource::Artist
                | PlaySource::Playlist
        ) && ev.played_ms >= 30_000
        {
            apply(&mut m, i, 1.0, s.manual, ev.ts, Some(&lv));
        }
    }
    // Likes: the latest like/unlike per track decides (a removed like is not a signal).
    let mut liked: HashMap<i64, (bool, i64)> = HashMap::new();
    for f in &h.feedback {
        let Some(t) = f.track_id else {
            continue;
        };
        if guests(&f.profile) {
            if f.kind == "like"
                && let Some(i) = cat.idx(t)
            {
                guest(&mut m, i, 1.0, s.like, f.ts);
            }
            continue;
        }
        match f.kind.as_str() {
            "like" => {
                liked.insert(t, (true, f.ts));
            }
            "unlike" => {
                liked.insert(t, (false, f.ts));
            }
            "never" => {
                if let Some(i) = cat.idx(t) {
                    // Hard exclusion lives in the catalog; here only the taste learns (§8.1).
                    let x = cat.feat[i];
                    m.taste.update(&x, 0.0, s.never);
                }
            }
            "not_now" => {
                if let (Some(i), Some(key)) = (cat.idx(t), f.ctx_key.as_deref())
                    && let Some(lv) = key_levels(key)
                {
                    // Only the context model (§8.1).
                    let x = cat.feat[i];
                    for l in lv {
                        let entry = m.context.entry(l).or_default();
                        entry.0.update(&x, 0.0, s.not_now);
                        entry.1 += 1;
                    }
                }
            }
            _ => {}
        }
    }
    // Only the last few skips repel (the events are in time order).
    let keep = sess.repel_last;
    if m.skips.len() > keep {
        m.skips.drain(..m.skips.len() - keep);
    }
    // In time order: the taste learns online, so the order matters (a HashMap's would not be
    // the same twice).
    let mut liked: Vec<(i64, (bool, i64))> = liked.into_iter().collect();
    liked.sort_by_key(|(t, (_, ts))| (*ts, *t));
    for (t, (on, ts)) in liked {
        if on && let Some(i) = cat.idx(t) {
            apply(&mut m, i, 1.0, s.like, ts, None);
            if let Some(k) = sess_decay(ts) {
                for ((sv, xv), cv) in m
                    .session
                    .iter_mut()
                    .zip(cat.feat[i].iter())
                    .zip(center.iter())
                {
                    *sv += sess.like_step * k * (xv - cv);
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth;
    use sirtone_core::wave::Daypart;

    const NOW: i64 = 1_790_000_000_000;

    fn ev(track: i64, reason: EndReason, played: u64, ts: i64) -> PlayEvent {
        PlayEvent {
            ts,
            track_id: track,
            played_ms: played,
            duration_ms: 200_000,
            end_reason: reason,
            source: PlaySource::Wave,
            profile: None,
            context: ContextSnapshot {
                now_utc_ms: ts,
                local_minute: 20 * 60,
                weekday: 2,
                daypart: Daypart::Evening,
                season_w: [0.0, 0.0, 0.0, 1.0],
                weather: None,
            },
            event_id: None,
            novelty: None,
            category: None,
        }
    }

    #[test]
    fn signals_move_the_models_as_the_table_says() {
        let mut tracks = synth::library(10, NOW, 1.0, 3);
        for t in &mut tracks {
            t.liked = false;
            t.never = false;
        }
        let cfg = Config::builtin().expect("assets");
        let cat = Catalog::build(&tracks, &cfg);
        let empty = replay(&cfg, &cat, &History::default(), NOW);
        assert!(
            empty
                .alpha
                .iter()
                .chain(empty.beta.iter())
                .all(|&v| v == 1.0)
        );
        assert!(empty.context.is_empty());

        let (a, b, c) = (tracks[0].id, tracks[1].id, tracks[2].id);
        let h = History {
            events: vec![
                ev(b, EndReason::Skip, 5_000, NOW - 1000), // early skip: 0.8
                ev(c, EndReason::Skip, 150_000, NOW - 1000), // late skip: 0.1
            ],
            feedback: vec![FeedbackRow {
                ts: NOW - 1000,
                track_id: Some(a),
                artist_id: None,
                kind: "like".into(),
                ctx_key: None,
                profile: None,
            }],
        };
        let m = replay(&cfg, &cat, &h, NOW);
        let mean = |id: i64| {
            let i = cat.idx(id).unwrap();
            m.alpha[i] / (m.alpha[i] + m.beta[i])
        };
        assert!(mean(a) > 0.5, "a like raises the track");
        assert!(
            mean(b) < mean(c) && mean(c) < 0.5,
            "an early skip weighs more than a late one"
        );
        // The evening context learnt from the skips.
        assert!(
            m.context
                .get(&format!("d{}", Daypart::Evening as u8))
                .is_some_and(|(_, n)| *n == 2)
        );
    }

    #[test]
    fn not_now_teaches_only_its_context() {
        let tracks = synth::library(5, NOW, 1.0, 4);
        let cfg = Config::builtin().expect("assets");
        let cat = Catalog::build(&tracks, &cfg);
        let h = History {
            events: vec![],
            feedback: vec![FeedbackRow {
                ts: NOW - 1000,
                track_id: Some(tracks[0].id),
                artist_id: None,
                kind: "not_now".into(),
                ctx_key: Some("d3/s3/wx".into()),
                profile: None,
            }],
        };
        let m = replay(&cfg, &cat, &h, NOW);
        let i = cat.idx(tracks[0].id).unwrap();
        assert_eq!((m.alpha[i], m.beta[i]), (1.0, 1.0), "no beta change");
        assert!(m.context.contains_key("d3") && m.context.contains_key("d3/s3/wx"));
    }
}
