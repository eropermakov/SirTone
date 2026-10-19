//! P-08 on the real repositories: 100 000 tracks, pages of 100 (budget ≤ 10 ms, S-15: 0.9 ms).
//! Slow to build the data → `#[ignore]`; run with
//! `cargo test --release -p sirtone-library --test scale -- --ignored --nocapture`.
#![allow(clippy::unwrap_used, clippy::print_stdout)]

use std::time::Instant;

use sirtone_library::collection::{albums_page, artists_page, counts};
use sirtone_library::repo::*;

fn p95(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[(v.len() as f64 * 0.95) as usize]
}

#[test]
#[ignore = "builds 100 000 tracks"]
fn pages_of_100_on_100k_tracks() {
    let dir = std::env::temp_dir().join(format!("sirtone-scale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut c =
        sirtone_library::open(&dir.join("library.db"), sirtone_library::Medium::Fixed).unwrap();
    let words = [
        "Ночь",
        "Город",
        "Звезда",
        "Highway",
        "Thunder",
        "Dream",
        "Море",
        "Ветер",
        "Light",
        "Сердце",
    ];
    let t0 = Instant::now();
    let mut artists = Vec::new();
    for i in 0..10_000 {
        artists.push(artist_get_or_create(&c, &format!("{} {i}", words[i % 10])).unwrap());
    }
    let mut albums = Vec::new();
    for i in 0..15_000 {
        albums.push(
            album_get_or_create(
                &c,
                &format!("{} {} {i}", words[i % 10], words[(i * 3) % 10]),
                artists[i % artists.len()],
                Some(2000),
            )
            .unwrap(),
        );
    }
    for i in 0..100_000usize {
        let a = artists[(i * 7) % artists.len()];
        track_insert(
            &mut c,
            &NewTrack {
                rel_path: format!("A{a}/B/{i:06}.flac"),
                size_bytes: 30_000_000,
                mtime_ms: 0,
                content_hash: format!("{i:064x}"),
                container: "flac".into(),
                codec: "flac".into(),
                lossless: true,
                sample_rate: Some(44100),
                channels: Some(2),
                duration_ms: 200_000,
                title: format!("{} {} {i}", words[(i * 3) % 10], words[(i * 7) % 10]),
                album_id: Some(albums[i % albums.len()]),
                disc_no: None,
                track_no: Some((i % 12) as u32 + 1),
                year: None,
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![a],
            },
        )
        .unwrap();
    }
    println!(
        "built 100 000 tracks in {:.1} s (one transaction per track)",
        t0.elapsed().as_secs_f64()
    );
    c.execute_batch("ANALYZE;").unwrap();

    let mut by_title = Vec::new();
    let mut cur = None;
    for _ in 0..300 {
        let t = Instant::now();
        let (rows, next) = tracks_by_title(&c, cur.as_ref(), 100).unwrap();
        by_title.push(t.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(rows.len(), 100);
        cur = next;
    }
    let mut art = Vec::new();
    let mut cur = None;
    for _ in 0..50 {
        let t = Instant::now();
        let (_, next) = artists_page(&c, cur.as_ref(), 100).unwrap();
        art.push(t.elapsed().as_secs_f64() * 1000.0);
        cur = next;
    }
    let mut alb = Vec::new();
    let mut cur = None;
    for _ in 0..50 {
        let t = Instant::now();
        let (_, next) = albums_page(&c, cur.as_ref(), 100).unwrap();
        alb.push(t.elapsed().as_secs_f64() * 1000.0);
        cur = next;
    }
    let t = Instant::now();
    let n = counts(&c).unwrap();
    let counts_ms = t.elapsed().as_secs_f64() * 1000.0;
    println!(
        "P-08 p95: tracks by title {:.2} ms, artists {:.2} ms, albums {:.2} ms; counts {:.1} ms ({} tracks)",
        p95(by_title.clone()),
        p95(art.clone()),
        p95(alb.clone()),
        counts_ms,
        n.tracks
    );
    assert!(
        p95(by_title) <= 10.0 && p95(art) <= 10.0 && p95(alb) <= 10.0,
        "P-08 exceeded"
    );
    // T3.15: search II on the same 100 000 tracks — every form (as typed, layout, translit),
    // stems, the typo fallback; `05` §9: ≤ 80 ms p95.
    let queries = [
        "ночь",
        "город зв",
        "звезда",
        "highway",
        "thunder dream",
        "сердце 5",
        "море ветер",
        "light 77",
        "dream 123",
        "highwy",
        "thundr",
        "lihgt",
        "dreem",
        "звзеда",
        "сердцо",
        "ujhjl",
        "vjht",
        "yjxm",
        "cthlwt",
        "gorod",
        "zvezda",
        "veter",
        "nochi",
        "serdtse",
        "сердца",
        "звезды",
        "городе",
        "ветра",
        "ночью",
        "морем",
        "Ночь 9",
        "ГОРОД",
        "dreams",
        "lights",
        "thunders",
        "high",
        "ве",
        "з",
        "highway 9999",
        "звезда 42",
    ];
    let mut search_ms = Vec::new();
    for _ in 0..5 {
        for q in queries {
            let t = Instant::now();
            let _ = sirtone_library::search::search(&c, q).unwrap();
            search_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let mut per: Vec<(f64, &str)> = queries
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let worst = (0..5)
                .map(|k| search_ms[k * queries.len() + i])
                .fold(0.0, f64::max);
            (worst, *q)
        })
        .collect();
    per.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("slowest: {:?}", &per[..8]);
    println!(
        "search II p95 {:.1} ms ({} queries)",
        p95(search_ms.clone()),
        search_ms.len()
    );
    assert!(p95(search_ms) <= 80.0, "search p95 exceeded");
    drop(c);
    let _ = std::fs::remove_dir_all(&dir);
}
