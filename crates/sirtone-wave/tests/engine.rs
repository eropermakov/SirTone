//! Engine properties (docs/14 §21): «никогда» never reaches the queue, artist spacing, the
//! repeat window, slider quotas, «не сейчас» only in its bucket.

use sirtone_core::wave::{ContextSnapshot, Daypart, NotNowRow};
use sirtone_wave::context::bucket_key;
use sirtone_wave::engine::{Config, Engine};
use sirtone_wave::synth;

const NOW: i64 = 1_790_000_000_000;

fn ctx(daypart: Daypart) -> ContextSnapshot {
    ContextSnapshot {
        now_utc_ms: NOW,
        local_minute: 20 * 60,
        weekday: 2,
        daypart,
        season_w: [0.0, 0.0, 0.0, 1.0],
        weather: None,
    }
}

#[test]
fn never_spacing_and_repeat_hold_over_many_rebuilds() {
    let tracks = synth::library(5_000, NOW, 0.5, 7);
    let cfg = Config::builtin().expect("assets");
    let spacing = cfg.profile.sequencing.artist_spacing;
    let mut e = Engine::new(cfg, &tracks, &synth::history(&tracks, NOW), NOW, 1);
    let never: std::collections::HashSet<i64> =
        tracks.iter().filter(|t| t.never).map(|t| t.id).collect();
    let artist: std::collections::HashMap<i64, i64> =
        tracks.iter().map(|t| (t.id, t.artist_id)).collect();
    let mut history: Vec<i64> = Vec::new();
    let mut now = NOW;
    for _ in 0..60 {
        for id in e.next(&ctx(Daypart::Evening), &[], 0.5, 5, now) {
            assert!(!never.contains(&id), "«никогда» {id} in the queue");
            assert!(!history.contains(&id), "{id} repeated within the window");
            let a = artist[&id];
            let recent = history.iter().rev().take(spacing - 1);
            assert!(
                recent.into_iter().all(|h| artist[h] != a),
                "artist {a} twice in {spacing}"
            );
            history.push(id);
            e.played(id, now);
            now += 200_000;
        }
    }
    assert_eq!(history.len(), 300);
}

#[test]
fn slider_quotas_are_kept() {
    let tracks = synth::library(8_000, NOW, 0.5, 11);
    for (novelty, want) in [
        (0.0f32, [0.90, 0.10, 0.00]),
        (0.5, [0.50, 0.20, 0.30]),
        (1.0, [0.15, 0.15, 0.70]),
    ] {
        let cfg = Config::builtin().expect("assets");
        let mut e = Engine::new(cfg, &tracks, &synth::history(&tracks, NOW), NOW, 3);
        let picks: Vec<i64> = (0..40)
            .flat_map(|_| e.next(&ctx(Daypart::Day), &[], novelty, 5, NOW))
            .collect();
        assert_eq!(picks.len(), 200);
        let mut counts = [0f32; 3];
        for id in &picks {
            counts[e.group_of(*id, NOW).expect("known track")] += 1.0;
        }
        for g in 0..3 {
            let share = counts[g] / picks.len() as f32;
            assert!(
                (share - want[g]).abs() < 0.08,
                "novelty {novelty}: group {g} {share:.2} vs {}",
                want[g]
            );
        }
    }
}

#[test]
fn not_now_blocks_only_in_its_bucket() {
    let tracks = synth::library(300, NOW, 1.0, 5);
    let evening = ctx(Daypart::Evening);
    // Shelve every track except a handful in the evening bucket.
    let shelf: Vec<NotNowRow> = tracks
        .iter()
        .skip(10)
        .map(|t| NotNowRow {
            track_id: t.id,
            ctx_key: bucket_key(&evening),
            expires_ms: NOW + 1_000_000,
        })
        .collect();
    let allowed: Vec<i64> = tracks.iter().take(10).map(|t| t.id).collect();
    let mut e = Engine::new(
        Config::builtin().expect("assets"),
        &tracks,
        &synth::history(&tracks, NOW),
        NOW,
        9,
    );
    for id in e.next(&evening, &shelf, 0.5, 5, NOW) {
        assert!(allowed.contains(&id), "{id} was shelved for the evening");
    }
    // In the morning the shelf does not apply.
    let mut e = Engine::new(
        Config::builtin().expect("assets"),
        &tracks,
        &synth::history(&tracks, NOW),
        NOW,
        9,
    );
    let morning = e.next(&ctx(Daypart::Morning), &shelf, 0.5, 5, NOW);
    assert!(morning.iter().any(|id| !allowed.contains(id)));
}

#[test]
fn why_explains_the_pick() {
    use sirtone_wave::engine::Why;
    let mut tracks = synth::library(400, NOW, 1.0, 21);
    // One fresh, never-played track and one liked, long-familiar track.
    tracks[0].play_count = 0;
    tracks[0].last_played_ms = None;
    tracks[0].added_ms = NOW - 86_400_000;
    tracks[1].liked = true;
    tracks[1].play_count = 9;
    tracks[1].last_played_ms = Some(NOW - 5 * 86_400_000);
    let history = synth::history(&tracks, NOW);
    let mut e = Engine::new(
        Config::builtin().expect("assets"),
        &tracks,
        &history,
        NOW,
        2,
    );
    let _ = e.next(&ctx(Daypart::Evening), &[], 0.5, 5, NOW);
    assert_eq!(e.why(tracks[0].id).first(), Some(&Why::NewForYou));
    let liked = e.why(tracks[1].id);
    assert!(!liked.is_empty() && liked.len() <= 2);
    assert!(liked.contains(&Why::Liked), "{liked:?}");
}
