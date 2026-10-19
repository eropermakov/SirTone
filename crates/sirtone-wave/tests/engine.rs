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

#[test]
fn seasonal_wave_follows_the_season() {
    // Synthetic titles: every 40th is «Winter Song N». Retitle a set as summer and tag a few.
    let mut tracks = synth::library(4_000, NOW, 0.0, 5);
    for t in tracks.iter_mut().filter(|t| t.id % 40 == 20) {
        t.title = Some(format!("Summer Days {}", t.id));
    }
    let winter: std::collections::HashSet<i64> = tracks
        .iter()
        .filter(|t| t.title.as_deref().is_some_and(|s| s.starts_with("Winter")))
        .map(|t| t.id)
        .collect();
    let summer: std::collections::HashSet<i64> = tracks
        .iter()
        .filter(|t| t.title.as_deref().is_some_and(|s| s.starts_with("Summer")))
        .map(|t| t.id)
        .collect();
    // «Убрать из сезонной» on some winter tracks.
    let untagged: std::collections::HashSet<i64> =
        winter.iter().copied().filter(|id| id % 80 == 0).collect();
    for t in tracks.iter_mut().filter(|t| untagged.contains(&t.id)) {
        t.season_tags = sirtone_wave::season::NO_SEASON;
    }
    let mut c = ctx(Daypart::Evening);
    c.season_w = [1.0, 0.0, 0.0, 0.0];
    let cfg = Config::builtin_profile("seasonal").expect("assets");
    assert_eq!(cfg.profile.id, "seasonal");
    let mut e = Engine::new(cfg, &tracks, &synth::history(&tracks, NOW), NOW, 3);
    let picks = e.next(&c, &[], 0.5, 40, NOW);
    let hits = picks.iter().filter(|id| winter.contains(id)).count();
    assert!(
        picks.iter().all(|id| !summer.contains(id)),
        "summer in a winter wave"
    );
    assert!(
        picks.iter().all(|id| !untagged.contains(id)),
        "«убрать из сезонной» picked"
    );
    // 2.5 % of the library is winter; the seasonal wave leans on it heavily.
    assert!(hits >= 10, "{hits} winter tracks of {}", picks.len());
    let reasons = e.why(
        *picks
            .iter()
            .find(|id| winter.contains(id))
            .expect("a winter pick"),
    );
    assert!(
        reasons.contains(&sirtone_wave::engine::Why::Season(0)),
        "{reasons:?}"
    );

    // Summer: no «Winter» title gets in; an artist named for another season is weak evidence
    // and stays eligible (the floor cuts only title/album words of another season).
    let mut summer_ctx = c.clone();
    summer_ctx.season_w = [0.0, 0.0, 1.0, 0.0];
    let mut e2 = Engine::new(
        Config::builtin_profile("seasonal").expect("assets"),
        &tracks,
        &synth::history(&tracks, NOW),
        NOW,
        9,
    );
    let summer_picks = e2.next(&summer_ctx, &[], 0.5, 40, NOW);
    assert!(
        summer_picks.iter().all(|id| !winter.contains(id)),
        "«Winter» title in summer"
    );
    // The floor sits between title/album and artist evidence for another season.
    let floor = Config::builtin_profile("seasonal")
        .expect("assets")
        .profile
        .season_floor
        .expect("seasonal floor");
    let s = sirtone_wave::season::Seasonality::builtin().expect("assets");
    let by_artist = s.affinity(
        0,
        &s.words(Some("Песня"), None, Some("Снег")),
        None,
        0.0,
        None,
    );
    let by_album = s.affinity(
        0,
        &s.words(Some("Песня"), Some("Зима"), None),
        None,
        0.0,
        None,
    );
    let by_title = s.affinity(0, &s.words(Some("Зима"), None, None), None, 0.0, None);
    assert!(
        by_artist[2] >= floor,
        "artist «Снег» out of summer: {by_artist:?}"
    );
    assert!(by_album[2] < floor && by_title[2] < floor);

    // «Моя волна»: season weighs less; summer tracks are not excluded, only lower.
    let cfg = Config::builtin().expect("assets");
    assert_eq!(cfg.profile.season_floor, None);
    let mut e = Engine::new(cfg, &tracks, &synth::history(&tracks, NOW), NOW, 3);
    let mine = e.next(&c, &[], 0.5, 40, NOW);
    let mine_hits = mine.iter().filter(|id| winter.contains(id)).count();
    assert!(mine_hits < hits, "my wave {mine_hits} vs seasonal {hits}");
}

mod query {
    use super::*;
    use sirtone_wave::query::{Activity, Character, Freq, Lang, Mood, WaveQuery};

    const DAY: i64 = 24 * 3600 * 1000;

    /// AT-08 (data): «энергичное + без слов + забытые + тренировка» — every pick keeps every
    /// filter that was not relaxed, and relaxation follows the profile's order.
    #[test]
    fn filters_hold_or_relax_in_order() {
        // Large: every filter holds; small: relaxation, in order, and the rest still hold.
        assert!(at08(8_000).is_empty());
        assert!(!at08(400).is_empty());
    }

    #[allow(clippy::expect_used)]
    fn at08(n: usize) -> Vec<&'static str> {
        let tracks = synth::library(n, NOW, 1.0, 21);
        let by_id: std::collections::HashMap<i64, &sirtone_core::wave::WaveTrack> =
            tracks.iter().map(|t| (t.id, t)).collect();
        let cfg = Config::builtin().expect("assets");
        let order: Vec<String> = cfg.profile.relax_order.clone();
        let mut e = Engine::new(cfg, &tracks, &synth::history(&tracks, NOW), NOW, 4);
        let q = WaveQuery {
            character: vec![Character::Energetic],
            language: vec![Lang::None],
            frequency: vec![Freq::Forgotten],
            activity: vec![Activity::Workout],
            ..WaveQuery::default()
        };
        let relaxed = e.set_query(q, NOW);
        // The steps are a subsequence of relax_order.
        let pos: Vec<usize> = relaxed
            .iter()
            .map(|r| {
                order
                    .iter()
                    .position(|o| o.trim_start_matches("filters.") == *r)
                    .expect("a configured step")
            })
            .collect();
        assert!(pos.windows(2).all(|w| w[0] < w[1]), "{relaxed:?}");
        let picks = e.next(&ctx(Daypart::Evening), &[], 0.5, 20, NOW);
        assert_eq!(picks.len(), 20);
        for id in &picks {
            let t = by_id[id];
            let x = t.features.expect("analysed").0;
            if !relaxed.contains(&"character") {
                assert!(x[0] >= 0.55, "energy {}", x[0]);
            }
            if !relaxed.contains(&"language") {
                assert!(x[9] < 0.2, "vocal {}", x[9]);
            }
            if !relaxed.contains(&"frequency") {
                let last = t.last_played_ms.expect("played");
                assert!(t.play_count >= 3 && NOW - last > 60 * DAY, "{t:?}");
            }
            if !relaxed.contains(&"activity") {
                let bpm = 50.0 + 150.0 * x[4];
                assert!((110.0..=190.0).contains(&bpm), "bpm {bpm}");
            }
        }
        relaxed
    }

    /// «Русский + часто слушаю» where «часто» is the bottleneck: only frequency is relaxed and
    /// every pick is still Russian.
    #[test]
    fn only_the_limiting_filter_is_relaxed() {
        let mut tracks = synth::library(2_000, NOW, 0.0, 25);
        for t in &mut tracks {
            t.plays_weeks = [0; 13];
            if t.id % 2 == 0 {
                t.title = Some(format!("Песня {}", t.id));
                t.artist_name = Some("Артист".into());
            }
            if t.id % 200 == 0 {
                t.last_played_ms = Some(NOW - 2 * DAY);
                t.plays_weeks[0] = 5;
            }
        }
        let ru: std::collections::HashSet<i64> = tracks
            .iter()
            .filter(|t| t.id % 2 == 0)
            .map(|t| t.id)
            .collect();
        let mut e = Engine::new(
            Config::builtin().expect("assets"),
            &tracks,
            &synth::history(&tracks, NOW),
            NOW,
            8,
        );
        let relaxed = e.set_query(
            WaveQuery {
                language: vec![Lang::Ru],
                frequency: vec![Freq::Often],
                ..WaveQuery::default()
            },
            NOW,
        );
        assert_eq!(relaxed, ["frequency"]);
        let picks = e.next(&ctx(Daypart::Evening), &[], 0.5, 20, NOW + 3 * DAY);
        assert_eq!(picks.len(), 20);
        assert!(picks.iter().all(|id| ru.contains(id)));
    }

    #[test]
    fn unanalysed_tracks_never_pass_a_mood_and_the_wave_relaxes() {
        let tracks = synth::library(3_000, NOW, 0.0, 22);
        let mut e = Engine::new(
            Config::builtin().expect("assets"),
            &tracks,
            &synth::history(&tracks, NOW),
            NOW,
            5,
        );
        let relaxed = e.set_query(
            WaveQuery {
                mood: vec![Mood::Happy],
                ..WaveQuery::default()
            },
            NOW,
        );
        assert_eq!(relaxed, ["mood"]);
        assert_eq!(e.next(&ctx(Daypart::Evening), &[], 0.5, 5, NOW).len(), 5);
    }

    /// T3.7: the tempo filter uses the measured BPM (B1) without deep analysis; the tempo feature
    /// is the measured one.
    #[test]
    fn tempo_filter_uses_measured_bpm() {
        let mut tracks = synth::library(2_000, NOW, 0.0, 26);
        for t in &mut tracks {
            t.bpm = Some(70.0 + (t.id % 110) as f32);
            t.key_camelot = Some(format!("{}B", t.id % 12 + 1));
        }
        let bpm: std::collections::HashMap<i64, f32> = tracks
            .iter()
            .map(|t| (t.id, t.bpm.unwrap_or(0.0)))
            .collect();
        let mut e = Engine::new(
            Config::builtin().expect("assets"),
            &tracks,
            &synth::history(&tracks, NOW),
            NOW,
            9,
        );
        let relaxed = e.set_query(
            WaveQuery {
                tempo: Some((120.0, 130.0)),
                ..WaveQuery::default()
            },
            NOW,
        );
        assert!(relaxed.is_empty(), "{relaxed:?}");
        let picks = e.next(&ctx(Daypart::Evening), &[], 0.5, 20, NOW);
        assert_eq!(picks.len(), 20);
        assert!(picks.iter().all(|id| (120.0..=130.0).contains(&bpm[id])));
    }

    #[test]
    fn never_is_not_relaxed() {
        let mut tracks = synth::library(200, NOW, 1.0, 23);
        for t in &mut tracks {
            t.never = true;
        }
        let mut e = Engine::new(
            Config::builtin().expect("assets"),
            &tracks,
            &synth::history(&tracks, NOW),
            NOW,
            6,
        );
        let relaxed = e.set_query(
            WaveQuery {
                mood: vec![Mood::Sad],
                language: vec![Lang::Ru],
                ..WaveQuery::default()
            },
            NOW,
        );
        assert!(!relaxed.is_empty());
        assert!(e.next(&ctx(Daypart::Evening), &[], 0.5, 5, NOW).is_empty());
    }

    #[test]
    fn frequency_switches_the_slider_off() {
        let tracks = synth::library(6_000, NOW, 0.5, 24);
        let mut e = Engine::new(
            Config::builtin().expect("assets"),
            &tracks,
            &synth::history(&tracks, NOW),
            NOW,
            7,
        );
        let relaxed = e.set_query(
            WaveQuery {
                frequency: vec![Freq::Loved],
                ..WaveQuery::default()
            },
            NOW,
        );
        assert!(relaxed.is_empty(), "{relaxed:?}");
        // Slider at «Неожиданное» (70 % new) — but only loved tracks pass, and the wave still
        // fills: the quotas are off.
        let picks = e.next(&ctx(Daypart::Evening), &[], 1.0, 30, NOW);
        assert_eq!(picks.len(), 30);
        assert!(picks.iter().all(|id| e.group_of(*id, NOW) == Some(0)));
    }
}

/// B3 (T3.9): the stored vector carries the vibe; a genre from the tags beats the model's, the
/// model's beats the library shares; mood bits come from the wave's own membership functions.
#[test]
fn deep_features_and_genre_precedence() {
    use sirtone_core::analysis::Vibe as Deep;
    use sirtone_wave::engine::{Catalog, deep_features, f};
    use sirtone_wave::query::Vibe;
    let v = Deep {
        energy: 0.9,
        valence: 0.85,
        arousal: 0.75,
        danceability: 0.8,
        acousticness: 0.1,
        vocal: 0.7,
        aggression: 0.1,
        brightness: 0.6,
        moods: [1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.2],
        genres: [0.1, 0.0, 0.9, 0.0, 0.0, 0.0],
        instrumental: false,
        conf: 1.0,
    };
    let x = deep_features(&v, Some(125.0));
    assert_eq!(x[f::ENERGY], 0.9);
    assert_eq!(x[f::TEMPO], 0.5);
    assert_eq!(x[f::GENRE + 2], 0.9);
    assert_eq!(x[f::MOOD + 7], 0.2);
    let vibe = Vibe::builtin().expect("vibe");
    // valence 0.85 / arousal 0.75 — «весёлое» and «позитивное», not «грустное».
    let bits = vibe.mood_bits(&x);
    assert_eq!(bits & 0b1, 0b1);
    assert_eq!(bits & 0b1000, 0);
    assert_eq!(bits & 0b10000, 0b10000);
    // Party: danceability 0.8, energy 0.9.
    assert!(vibe.activities(&x)[5] >= 0.99);

    let mut tracks = synth::library(3, NOW, 0.0, 3);
    tracks[0].genre = Some("rock".into());
    tracks[0].features = Some((x, 1.0));
    tracks[1].genre = None;
    tracks[1].features = Some((x, 1.0));
    tracks[2].genre = None;
    tracks[2].features = None;
    let cfg = Config::builtin().expect("assets");
    let cat = Catalog::build(&tracks, &cfg);
    let genres = |i: usize| &cat.feat[i][f::GENRE..f::GENRE + f::GENRES];
    assert_eq!(genres(0), &[0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(genres(1), &v.genres);
    // Shares of the library: the one known genre is rock.
    assert_eq!(genres(2), &[0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
}

/// AT-20 (T3.32, docs/14 §16): «Для всех» plays only tracks that pass its exclusions —
/// no sad, gloomy or aggressive ones, nothing «не для компании»; two skips there change only
/// the guests' model, and the owner's models stay bit for bit the same.
#[test]
fn for_all_excludes_and_learns_apart() {
    use sirtone_core::wave::{EndReason, PlayEvent, PlaySource};
    use sirtone_wave::engine::Catalog;
    use sirtone_wave::learn::replay;
    use sirtone_wave::query::{Character, Mood, Vibe};

    let mut tracks = synth::library(2_000, NOW, 1.0, 21);
    // A «Хит для всех» that would otherwise be excluded, and a «не для компании».
    let hit = tracks[0].id;
    tracks[0].company = 1;
    if let Some((x, _)) = tracks[0].features.as_mut() {
        x[1] = 0.1;
        x[2] = 0.1;
    }
    let banned = tracks[1].id;
    tracks[1].company = -1;
    let history = synth::history(&tracks, NOW);
    let cfg = Config::builtin_profile("for_all").expect("assets");
    assert_eq!(cfg.profile.id, "for_all");
    let vibe = Vibe::builtin().expect("vibe");
    let mut e = Engine::new(cfg.clone(), &tracks, &history, NOW, 5);
    let mut seen = std::collections::HashSet::new();
    for i in 0..60 {
        for id in e.next(&ctx(Daypart::Evening), &[], 0.5, 5, NOW + i * 60_000) {
            seen.insert(id);
        }
    }
    assert!(seen.len() >= 100, "{}", seen.len());
    assert!(!seen.contains(&banned));
    let feat = |id: i64| {
        tracks
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.features)
            .map(|f| f.0)
            .expect("analysed")
    };
    for id in &seen {
        if *id == hit {
            continue;
        }
        let x = feat(*id);
        assert!(
            !vibe.excluded(&x, &[Mood::Sad, Mood::Negative], &[Character::Aggressive]),
            "{id} {x:?}"
        );
    }
    // Two skips in «Для всех»: only the guests' model moves.
    let cat = Catalog::build(&tracks, &cfg);
    let my = Config::builtin().expect("assets");
    let before = replay(&my, &cat, &history, NOW);
    let mut h2 = history.clone();
    let victim = tracks[5].id;
    for k in 0..2 {
        let mut ev: PlayEvent = h2.events[0].clone();
        ev.track_id = victim;
        ev.ts = NOW - 1_000 * (k + 1);
        ev.played_ms = 5_000;
        ev.end_reason = EndReason::Skip;
        ev.source = PlaySource::Wave;
        ev.profile = Some("for_all".into());
        h2.events.push(ev);
    }
    let after = replay(&my, &cat, &h2, NOW);
    let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&before.alpha), bits(&after.alpha));
    assert_eq!(bits(&before.beta), bits(&after.beta));
    assert_eq!(format!("{:?}", before.taste), format!("{:?}", after.taste));
    let mut ck: Vec<_> = before.context.keys().cloned().collect();
    ck.sort();
    for k in &ck {
        assert_eq!(
            format!("{:?}", before.context[k]),
            format!("{:?}", after.context[k])
        );
    }
    assert_eq!(before.context.len(), after.context.len());
    assert_eq!(bits(&before.session), bits(&after.session));
    let vi = tracks.iter().position(|t| t.id == victim).expect("victim");
    assert!(after.guest_beta[vi] > before.guest_beta[vi]);
}

/// T3.25: every built-in profile loads; an invalid one is a clear error naming the file.
#[test]
fn profiles_validate() {
    for id in ["my_wave", "seasonal", "for_all"] {
        assert_eq!(Config::builtin_profile(id).expect(id).profile.id, id);
    }
    let base = include_str!("../../../assets/wave/profiles/my_wave.json");
    let cases = [
        ("{", "user.json"),
        (r#"{"schema":"other"}"#, "schema"),
        (&base.replace(r#""taste": 0.45"#, r#""taste": -1"#), "taste"),
        (
            &base.replace(r#""learning_policy": "full""#, r#""learning_policy": "x""#),
            "learning_policy",
        ),
    ];
    for (text, needle) in cases {
        let e = Config::with_profile("user.json", text).expect_err("refused");
        let msg = e.to_string();
        assert!(msg.contains("user.json") && msg.contains(needle), "{msg}");
    }
}

/// AT-21 (T3.28 with the vibe of T3.9): three skips of sad tracks in a row — the next picks of
/// the wave hold fewer sad ones (the session model and the taste learn at once).
#[test]
fn three_sad_skips_turn_the_wave_away_from_sad() {
    use sirtone_core::wave::{EndReason, PlaySource};
    use sirtone_wave::query::{Mood, Vibe};

    let tracks = synth::library(5_000, NOW, 1.0, 77);
    let vibe = Vibe::builtin().expect("vibe");
    let sad = |id: i64| {
        tracks
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.features)
            .is_some_and(|(x, _)| vibe.excluded(&x, &[Mood::Sad], &[]))
    };
    let share = |history: &sirtone_wave::learn::History, seed: u64| {
        let mut e = Engine::new(
            Config::builtin().expect("assets"),
            &tracks,
            history,
            NOW,
            seed,
        );
        let mut n = 0;
        let mut s = 0;
        for i in 0..40 {
            for id in e.next(&ctx(Daypart::Evening), &[], 0.5, 5, NOW + i * 60_000) {
                n += 1;
                s += usize::from(sad(id));
            }
        }
        s as f32 / n.max(1) as f32
    };
    let history = synth::history(&tracks, NOW);
    let before: f32 = (0..4).map(|k| share(&history, 11 + k)).sum::<f32>() / 4.0;
    // Three sad tracks skipped early, a minute apart, just now.
    let mut h2 = history.clone();
    let sad_ids: Vec<i64> = tracks
        .iter()
        .map(|t| t.id)
        .filter(|id| sad(*id))
        .take(3)
        .collect();
    assert_eq!(sad_ids.len(), 3);
    for (k, id) in sad_ids.iter().enumerate() {
        let mut ev = h2.events[0].clone();
        ev.track_id = *id;
        ev.ts = NOW - 60_000 * (3 - k as i64);
        ev.played_ms = 8_000;
        ev.end_reason = EndReason::Skip;
        ev.source = PlaySource::Wave;
        ev.profile = Some("my_wave".into());
        h2.events.push(ev);
    }
    let after: f32 = (0..4).map(|k| share(&h2, 11 + k)).sum::<f32>() / 4.0;
    assert!(before > 0.05, "the library has sad tracks: {before}");
    assert!(after < before * 0.5, "sad share {before:.3} → {after:.3}");
    // AT-21: the next 10 tracks of the session — at most one sad (the mean over seeds).
    let next10 = |seed: u64| {
        let mut e = Engine::new(Config::builtin().expect("assets"), &tracks, &h2, NOW, seed);
        let mut ids = e.next(&ctx(Daypart::Evening), &[], 0.5, 5, NOW);
        ids.extend(e.next(&ctx(Daypart::Evening), &[], 0.5, 5, NOW + 60_000));
        ids.iter().filter(|id| sad(**id)).count()
    };
    let mean10 = (0..8).map(|k| next10(100 + k)).sum::<usize>() as f32 / 8.0;
    assert!(mean10 <= 1.0, "sad in the next 10: {mean10}");
}

/// D-92: a skip reported live repels too (the journal writes later), and a skipped track
/// without analysis repels nothing (its features are the library's means).
#[test]
fn live_skips_repel_and_unanalysed_ones_do_not() {
    use sirtone_wave::query::{Mood, Vibe};
    let tracks = synth::library(5_000, NOW, 0.6, 91);
    let history = synth::history(&tracks, NOW);
    let vibe = Vibe::builtin().expect("vibe");
    let sad = |id: i64| {
        tracks
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.features)
            .is_some_and(|(x, _)| vibe.excluded(&x, &[Mood::Sad], &[]))
    };
    let picks = |skips: &[i64], seed: u64| {
        let mut e = Engine::new(
            Config::builtin().expect("assets"),
            &tracks,
            &history,
            NOW,
            seed,
        );
        for (k, id) in skips.iter().enumerate() {
            e.skipped(*id, NOW - 60_000 * (skips.len() - k) as i64);
        }
        let mut ids = e.next(&ctx(Daypart::Evening), &[], 0.5, 5, NOW);
        ids.extend(e.next(&ctx(Daypart::Evening), &[], 0.5, 5, NOW + 60_000));
        ids
    };
    // An unanalysed skip: the very same picks.
    let unanalysed = tracks
        .iter()
        .find(|t| t.features.is_none())
        .expect("one")
        .id;
    for seed in 0..4 {
        assert_eq!(picks(&[], 300 + seed), picks(&[unanalysed], 300 + seed));
    }
    // Three sad skips reported live: ≤ 1 sad in the next 10 (mean over seeds).
    let sad_ids: Vec<i64> = tracks
        .iter()
        .map(|t| t.id)
        .filter(|id| sad(*id))
        .take(3)
        .collect();
    let mean: f32 = (0..8)
        .map(|k| {
            picks(&sad_ids, 400 + k)
                .iter()
                .filter(|id| sad(**id))
                .count() as f32
        })
        .sum::<f32>()
        / 8.0;
    assert!(mean <= 1.0, "sad in the next 10: {mean}");
}
