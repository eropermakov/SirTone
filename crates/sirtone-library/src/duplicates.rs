//! Duplicates by sound (T3.5, FR-DUP-001…004; `05` §6, `01b` F-09 — SongKong / dupeGuru by
//! content, beets actions). Exact copies (equal BLAKE3) never enter the library — import marks
//! them `duplicate`. Here: pairs found by fingerprints once a track is analysed:
//!
//! - **тот же звук** — similarity ≥ 0.92 over ≥ 80 % of the shorter one, |Δ duration| ≤ 3 s;
//! - **похожий (другая версия)** — similarity 0.75–0.92, or the same artist and title with a
//!   different sound (remix, live).
//!
//! Thresholds from S-06 (different songs topped at 0.682). Fingerprints that say too little
//! (silence, a drone) are never compared. Decisions: «оставить оба» (remembered), or one track
//! [`merge`]d into the other — its plays, journal, ♡, tags and playlist places move over, then
//! the caller sends the dropped file to the Recycle Bin through the journal.

use rusqlite::{Connection, OptionalExtension, params};
use sirtone_core::fingerprint::{
    decompress, informative, similarity, similarity_coarse, sketch_items,
};

use crate::collection::{TRACK_COLS, track_row};
use crate::db::now_ms;
use crate::error::LibraryError;
use crate::repo::{TrackId, TrackRow};
use crate::stats::WEEKS;

pub const SAME_SOUND: f32 = 0.92;
pub const SIMILAR: f32 = 0.75;
const MIN_OVERLAP: f32 = 0.8;
const DURATION_MS: i64 = 3_000;
const MIN_DISTINCT: usize = 16;
/// Coarse pre-check below which two fingerprints are surely different songs.
const COARSE: f32 = 0.70;
/// Candidates per new track at most (the duration window can be crowded).
const MAX_CANDIDATES: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    SameSound,
    Similar,
}

impl Class {
    fn as_str(self) -> &'static str {
        match self {
            Self::SameSound => "same_sound",
            Self::Similar => "similar",
        }
    }
}

/// The class of two fingerprints with their durations, if they are duplicates.
pub fn classify(a: &[u32], b: &[u32], dur_a: i64, dur_b: i64) -> Option<(Class, f32)> {
    if !informative(a, MIN_DISTINCT) || !informative(b, MIN_DISTINCT) {
        return None;
    }
    let (sim, overlap) = similarity(a, b)?;
    if overlap < MIN_OVERLAP {
        return None;
    }
    if sim >= SAME_SOUND && (dur_a - dur_b).abs() <= DURATION_MS {
        Some((Class::SameSound, sim))
    } else if sim >= SIMILAR {
        Some((Class::Similar, sim))
    } else {
        None
    }
}

fn store(
    conn: &Connection,
    x: TrackId,
    y: TrackId,
    class: Class,
    sim: f32,
) -> Result<bool, LibraryError> {
    let (a, b) = if x < y { (x, y) } else { (y, x) };
    let n = conn.execute(
        "INSERT INTO duplicate_pair(a, b, class, similarity, found_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(a, b) DO UPDATE SET class = excluded.class, similarity = excluded.similarity
         WHERE duplicate_pair.decision IS NULL AND duplicate_pair.class <> excluded.class",
        params![a, b, class.as_str(), f64::from(sim), now_ms()],
    )?;
    Ok(n > 0)
}

/// Looks for duplicates of a freshly fingerprinted track; returns how many pairs are new.
pub fn detect(conn: &Connection, track: TrackId) -> Result<u32, LibraryError> {
    let Some((blob, dur, title, artist)): Option<(Vec<u8>, i64, String, Option<String>)> = conn
        .query_row(
            "SELECT t.fingerprint, t.duration_ms, t.title_norm,
                    (SELECT a.name_norm FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                     WHERE ta.track_id = t.id ORDER BY ta.position LIMIT 1)
             FROM track t WHERE t.id = ?1 AND t.state = 'ok' AND t.fingerprint IS NOT NULL",
            [track],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
    else {
        return Ok(0);
    };
    let Some(fp) = decompress(&blob) else {
        return Ok(0);
    };
    let mut found = 0;
    // 1. Same sound or a close version: the duration window (index `ix_track_dur`); the
    // candidates' sketches first — only the few that look alike are decompressed.
    let mut st = conn.prepare_cached(
        "SELECT id, fp_sketch, duration_ms FROM track
         WHERE duration_ms BETWEEN ?2 AND ?3 AND id <> ?1 AND state = 'ok'
           AND fingerprint IS NOT NULL LIMIT ?4",
    )?;
    let mut full = conn.prepare_cached("SELECT fingerprint FROM track WHERE id = ?1")?;
    let mut rows = st.query(params![
        track,
        dur - DURATION_MS,
        dur + DURATION_MS,
        MAX_CANDIDATES as i64
    ])?;
    let mut seen = Vec::new();
    while let Some(r) = rows.next()? {
        let other: TrackId = r.get(0)?;
        // Tracks fingerprinted before sketches existed are compared in full.
        if let Some(sk) = r.get::<_, Option<Vec<u8>>>(1)?
            && similarity_coarse(&fp, &sketch_items(&sk)) < COARSE
        {
            continue;
        }
        let blob: Vec<u8> = full.query_row([other], |r| r.get(0))?;
        let Some(ofp) = decompress(&blob) else {
            continue;
        };
        if let Some((class, sim)) = classify(&fp, &ofp, dur, r.get(2)?) {
            seen.push(other);
            if store(conn, track, other, class, sim)? {
                found += 1;
            }
        }
    }
    drop(rows);
    // 2. The same artist and title, any length, another sound — another version. Only when
    // both fingerprints say enough: without that the sound is unknown, not «different».
    if let Some(artist) = artist
        && informative(&fp, MIN_DISTINCT)
    {
        let mut st = conn.prepare_cached(
            "SELECT t.id, t.fingerprint FROM track t
             WHERE t.title_norm = ?2 AND t.id <> ?1 AND t.state = 'ok' AND t.fingerprint IS NOT NULL
               AND EXISTS (SELECT 1 FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                           WHERE ta.track_id = t.id AND a.name_norm = ?3)",
        )?;
        let others: Vec<(TrackId, Vec<u8>)> = st
            .query_map(params![track, title, artist], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<Result<_, _>>()?;
        for (other, blob) in others {
            if seen.contains(&other) {
                continue;
            }
            let Some(ofp) = decompress(&blob) else {
                continue;
            };
            if informative(&ofp, MIN_DISTINCT) && store(conn, track, other, Class::Similar, 0.0)? {
                found += 1;
            }
        }
    }
    Ok(found)
}

/// A pair waiting for a decision, with what the choice needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Pair {
    pub class: Class,
    pub similarity: f32,
    pub a: Side,
    pub b: Side,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Side {
    pub track: TrackRow,
    pub codec: String,
    pub lossless: bool,
    pub bitrate_kbps: Option<u32>,
    pub size_bytes: u64,
    pub plays: u32,
}

/// Quality order (`05` §6): lossless > lossy, then bitrate, then size.
pub fn better(x: &Side, y: &Side) -> bool {
    (x.lossless, x.bitrate_kbps.unwrap_or(0), x.size_bytes)
        > (y.lossless, y.bitrate_kbps.unwrap_or(0), y.size_bytes)
}

fn side(conn: &Connection, id: TrackId) -> Result<Option<Side>, LibraryError> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {TRACK_COLS}, t.codec, t.lossless, t.bitrate_kbps, t.size_bytes, t.play_count
                 FROM track t LEFT JOIN album al ON al.id = t.album_id WHERE t.id = ?1"
            ),
            [id],
            |r| {
                Ok(Side {
                    track: track_row(r)?,
                    codec: r.get(8)?,
                    lossless: r.get(9)?,
                    bitrate_kbps: r.get(10)?,
                    size_bytes: r.get::<_, i64>(11)?.max(0) as u64,
                    plays: r.get(12)?,
                })
            },
        )
        .optional()?)
}

/// Pairs without a decision, «тот же звук» first.
pub fn pending(conn: &Connection) -> Result<Vec<Pair>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT p.a, p.b, p.class, p.similarity FROM duplicate_pair p
         JOIN track ta ON ta.id = p.a JOIN track tb ON tb.id = p.b
         WHERE p.decision IS NULL AND ta.state = 'ok' AND tb.state = 'ok'
         ORDER BY p.class = 'similar', p.similarity DESC, p.b DESC LIMIT 500",
    )?;
    let raw: Vec<(TrackId, TrackId, String, f64)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::with_capacity(raw.len());
    for (a, b, class, sim) in raw {
        if let (Some(a), Some(b)) = (side(conn, a)?, side(conn, b)?) {
            out.push(Pair {
                class: if class == "same_sound" {
                    Class::SameSound
                } else {
                    Class::Similar
                },
                similarity: sim as f32,
                a,
                b,
            });
        }
    }
    Ok(out)
}

pub fn pending_count(conn: &Connection) -> Result<u32, LibraryError> {
    Ok(conn.query_row(
        "SELECT count(*) FROM duplicate_pair p JOIN track ta ON ta.id = p.a
         JOIN track tb ON tb.id = p.b
         WHERE p.decision IS NULL AND ta.state = 'ok' AND tb.state = 'ok'",
        [],
        |r| r.get(0),
    )?)
}

/// «Оставить оба»: the pair is not offered again.
pub fn keep_both(conn: &Connection, x: TrackId, y: TrackId) -> Result<(), LibraryError> {
    let (a, b) = if x < y { (x, y) } else { (y, x) };
    conn.execute(
        "UPDATE duplicate_pair SET decision = 'keep_both' WHERE a = ?1 AND b = ?2",
        params![a, b],
    )?;
    Ok(())
}

/// Weekly rings merged on the later week (bucket `i` = week of the last play − i).
fn merge_weeks(a: &[u8], last_a: Option<i64>, b: &[u8], last_b: Option<i64>) -> Vec<u8> {
    const WEEK: i64 = 7 * 24 * 3600 * 1000;
    let la = last_a.map(|t| t.div_euclid(WEEK));
    let lb = last_b.map(|t| t.div_euclid(WEEK));
    let Some(top) = la.max(lb) else {
        return vec![0; WEEKS];
    };
    let mut out = vec![0u8; WEEKS];
    for (ring, last) in [(a, la), (b, lb)] {
        let Some(last) = last else { continue };
        let shift = (top - last).max(0) as usize;
        for (i, v) in ring.iter().take(WEEKS).enumerate() {
            if i + shift < WEEKS {
                out[i + shift] = out[i + shift].saturating_add(*v);
            }
        }
    }
    out
}

/// One track takes the other's place («Заменить старый новым» = `merge(new, old)`, «Не
/// добавлять новый» = `merge(old, new)`, FR-DUP-003/004), in one transaction: plays and skips
/// summed, the later last play, the weekly rings merged, «никогда» kept; the play journal, ♡ /
/// «не сейчас», tags and playlist places move to `keep`. The `drop` row stays (its file goes
/// to the Recycle Bin through the journal — the caller's job; then the row is removed).
pub fn merge(conn: &mut Connection, keep: TrackId, drop: TrackId) -> Result<(), LibraryError> {
    if keep == drop {
        return Err(LibraryError::Invalid("merge a track into itself".into()));
    }
    let tx = conn.transaction()?;
    type Agg = (i64, i64, Option<i64>, f64, Option<Vec<u8>>, i64);
    let read = |id: TrackId| -> Result<Agg, LibraryError> {
        tx.query_row(
            "SELECT play_count, skip_count, last_played_at, completion_ema, plays_recent,
                    wave_never FROM track WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| LibraryError::Invalid(format!("no track {id}")))
    };
    let k = read(keep)?;
    let d = read(drop)?;
    let plays = k.0 + d.0;
    let completion = if plays > 0 {
        (k.3 * k.0 as f64 + d.3 * d.0 as f64) / plays as f64
    } else {
        k.3.max(d.3)
    };
    let weeks = merge_weeks(
        k.4.as_deref().unwrap_or(&[]),
        k.2,
        d.4.as_deref().unwrap_or(&[]),
        d.2,
    );
    tx.execute(
        "UPDATE track SET play_count = ?2, skip_count = ?3, last_played_at = ?4,
                completion_ema = ?5, plays_recent = ?6, wave_never = ?7 WHERE id = ?1",
        params![
            keep,
            plays,
            k.1 + d.1,
            k.2.max(d.2),
            completion,
            (k.2.is_some() || d.2.is_some()).then_some(weeks),
            i64::from(k.5 != 0 || d.5 != 0),
        ],
    )?;
    for sql in [
        "UPDATE play_event SET track_id = ?1 WHERE track_id = ?2",
        "UPDATE feedback SET track_id = ?1 WHERE track_id = ?2",
        "UPDATE playlist_item SET track_id = ?1 WHERE track_id = ?2",
        "INSERT OR IGNORE INTO track_tag(track_id, tag, value, created_at)
         SELECT ?1, tag, value, created_at FROM track_tag WHERE track_id = ?2",
        "INSERT INTO play_event_rollup(track_id, year, season, daypart, plays, skips, played_ms)
         SELECT ?1, year, season, daypart, plays, skips, played_ms FROM play_event_rollup
         WHERE track_id = ?2 AND true
         ON CONFLICT(track_id, year, season, daypart) DO UPDATE SET
           plays = plays + excluded.plays, skips = skips + excluded.skips,
           played_ms = played_ms + excluded.played_ms",
        "DELETE FROM duplicate_pair WHERE a IN (?1, ?2) AND b IN (?1, ?2)",
    ] {
        tx.execute(sql, params![keep, drop])?;
    }
    // A season tag and «убрать из сезонной» exclude each other (feedback.rs): the kept
    // track's own choice wins.
    let has_season: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM track_tag WHERE track_id = ?1 AND tag LIKE 'season_%')
            AND EXISTS (SELECT 1 FROM track_tag WHERE track_id = ?1 AND tag = 'no_season')",
        [keep],
        |r| r.get(0),
    )?;
    if has_season {
        tx.execute(
            "DELETE FROM track_tag WHERE track_id = ?1 AND tag = 'no_season'",
            [keep],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Exact copies the imports skipped (the third class of the report, `05` §6): source paths.
pub fn exact_copies(conn: &Connection, limit: u32) -> Result<Vec<String>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT src_path FROM import_item WHERE state = 'duplicate' ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = st
        .query_map([limit], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open;
    use crate::repo::{Identity, NewTrack, artist_get_or_create, track_insert};
    use sirtone_core::fingerprint::{compress, sketch};

    fn lib(tag: &str) -> Connection {
        let root = std::env::temp_dir().join(format!("sirtone-dup-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap()
    }

    fn melody(seed: u32, n: usize) -> Vec<u32> {
        let mut x = seed.wrapping_mul(2_654_435_761) | 1;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x
            })
            .collect()
    }

    /// The same items with `flips` random bits changed per item (another encoding).
    fn noisy(fp: &[u32], flips: u32, seed: u32) -> Vec<u32> {
        let mut r = seed | 1;
        fp.iter()
            .map(|&v| {
                let mut v = v;
                for _ in 0..flips {
                    r ^= r << 13;
                    r ^= r >> 17;
                    r ^= r << 5;
                    v ^= 1 << (r % 32);
                }
                v
            })
            .collect()
    }

    fn track(c: &mut Connection, n: u32, title: &str, artist: &str, dur: u64, fp: &[u32]) -> i64 {
        let a = artist_get_or_create(c, artist).unwrap();
        let id = track_insert(
            c,
            &NewTrack {
                rel_path: format!("A/{n:03}.mp3"),
                size_bytes: 1_000 + u64::from(n),
                mtime_ms: 0,
                content_hash: format!("h{n}"),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: dur,
                title: title.into(),
                album_id: None,
                disc_no: None,
                track_no: None,
                year: None,
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![a],
            },
        )
        .unwrap();
        c.execute(
            "UPDATE track SET fingerprint = ?2, fp_sketch = ?3 WHERE id = ?1",
            params![id, compress(fp, 1), sketch(fp)],
        )
        .unwrap();
        id
    }

    #[test]
    fn classes_and_silence() {
        let m = melody(1, 900);
        // Another encoding: ~1 bit of 32 differs → ≈ 0.97.
        let (c, s) = classify(&m, &noisy(&m, 1, 7), 200_000, 201_000).unwrap();
        assert_eq!(c, Class::SameSound, "{s}");
        // Same sound but 10 s longer → not «тот же звук».
        assert_eq!(
            classify(&m, &noisy(&m, 1, 7), 200_000, 210_000).unwrap().0,
            Class::Similar
        );
        // ~5 bits differ → ≈ 0.85: another version.
        assert_eq!(
            classify(&m, &noisy(&m, 5, 9), 200_000, 200_000).unwrap().0,
            Class::Similar
        );
        // Different songs.
        assert!(classify(&m, &melody(2, 900), 200_000, 200_000).is_none());
        // Two different near-silent tracks look identical — never compared.
        let quiet = vec![0u32; 900];
        let drone = vec![0x0F0F_0000u32; 900];
        assert!(classify(&quiet, &quiet, 200_000, 200_000).is_none());
        assert!(classify(&drone, &drone, 200_000, 200_000).is_none());
    }

    /// No false «тот же звук» among different random songs (`05` §13: < 1 %, on synthetic data
    /// until the golden set, OWN-08).
    #[test]
    fn no_false_pairs_among_different_songs() {
        let songs: Vec<Vec<u32>> = (0..120).map(|s| melody(s + 10, 900)).collect();
        let mut false_pairs = 0;
        for i in 0..songs.len() {
            for j in i + 1..songs.len() {
                if classify(&songs[i], &songs[j], 200_000, 200_000).is_some() {
                    false_pairs += 1;
                }
            }
        }
        assert_eq!(false_pairs, 0);
    }

    #[test]
    fn detect_list_keep_both_and_merge() {
        let mut c = lib("flow");
        let m = melody(3, 900);
        let old = track(&mut c, 1, "Утро", "Мелодия", 200_000, &m);
        let new = track(
            &mut c,
            2,
            "Рассвет (копия)",
            "Неизвестный",
            201_000,
            &noisy(&m, 1, 3),
        );
        let other = track(&mut c, 3, "Вечер", "Мелодия", 200_500, &melody(4, 900));
        let live = track(&mut c, 4, "Утро", "Мелодия", 260_000, &melody(5, 900));
        // Same name but a drone (sound unknown) — not «another version».
        let drone = track(&mut c, 5, "Утро", "Мелодия", 90_000, &[7u32; 900]);
        assert_eq!(detect(&c, new).unwrap(), 1);
        assert_eq!(
            detect(&c, live).unwrap(),
            1,
            "same name, other sound → version"
        );
        assert_eq!(detect(&c, other).unwrap(), 0);
        assert_eq!(detect(&c, drone).unwrap(), 0, "drone: sound unknown");
        assert_eq!(detect(&c, new).unwrap(), 0, "found once");
        let p = pending(&c).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].class, Class::SameSound);
        assert_eq!((p[0].a.track.id, p[0].b.track.id), (old, new));
        keep_both(&c, live, old).unwrap();
        assert_eq!(pending_count(&c).unwrap(), 1);
        detect(&c, live).unwrap();
        assert_eq!(
            pending_count(&c).unwrap(),
            1,
            "«оставить оба» is remembered"
        );

        // History of the old one: plays, a like, a tag, a playlist place, journal rows.
        c.execute(
            "UPDATE track SET play_count = 5, skip_count = 1, last_played_at = 1000,
                    completion_ema = 0.8, plays_recent = x'0302' WHERE id = ?1",
            [old],
        )
        .unwrap();
        crate::feedback::set_tag(&mut c, old, "season_summer", true).unwrap();
        c.execute(
            "INSERT INTO feedback(ts, track_id, kind) VALUES (1, ?1, 'like')",
            [old],
        )
        .unwrap();
        let pl = crate::playlists::create(&c, "Мой").unwrap();
        crate::playlists::add(&mut c, pl, &[old]).unwrap();
        c.execute(
            "INSERT INTO play_event(ts, track_id, played_ms, duration_ms, end_reason, source,
               local_minute, weekday, daypart, season_w)
             VALUES (1, ?1, 100000, 200000, 'finished', 'wave', 0, 0, 0, x'00000000')",
            [old],
        )
        .unwrap();
        // «Заменить старый новым».
        merge(&mut c, new, old).unwrap();
        let (plays, skips, last): (i64, i64, Option<i64>) = c
            .query_row(
                "SELECT play_count, skip_count, last_played_at FROM track WHERE id = ?1",
                [new],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((plays, skips, last), (5, 1, Some(1000)));
        assert!(crate::feedback::track_state(&c, new).unwrap().liked);
        assert_eq!(
            crate::feedback::track_tags(&c, new).unwrap(),
            ["season_summer"]
        );
        let items: Vec<i64> = crate::playlists::tracks(&c, pl)
            .unwrap()
            .into_iter()
            .map(|t| t.track.id)
            .collect();
        assert_eq!(items, [new]);
        let journal: i64 = c
            .query_row(
                "SELECT count(*) FROM play_event WHERE track_id = ?1",
                [new],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(journal, 1);
        assert_eq!(pending_count(&c).unwrap(), 0);
        // The dropped row loses nothing more when it is removed.
        crate::repo::track_remove(&mut c, old).unwrap();
        assert!(crate::feedback::track_state(&c, new).unwrap().liked);
    }

    /// `cargo test --release -p sirtone-library dup_crowded -- --ignored --nocapture`: one detection
    /// against 2 000 fingerprinted tracks in the ±3 s window (durations bunch around 3–4 min).
    #[test]
    #[ignore = "timing, release build"]
    fn dup_crowded() {
        let mut c = lib("crowd");
        for n in 0..2_000u32 {
            track(
                &mut c,
                n,
                &format!("Песня {n}"),
                "Группа",
                200_000 + u64::from(n % 5_000),
                &melody(n + 100, 900),
            );
        }
        let new = track(&mut c, 9_999, "Новая", "Другая", 201_000, &melody(7, 900));
        let t = std::time::Instant::now();
        let found = detect(&c, new).unwrap();
        let ms = t.elapsed().as_secs_f64() * 1e3;
        eprintln!("dup detect vs 2000 candidates: {ms:.1} ms, {found} found");
        let blobs: Vec<Vec<u8>> = (0..2_000u32)
            .map(|n| compress(&melody(n + 100, 900), 1))
            .collect();
        let t = std::time::Instant::now();
        let n: usize = blobs
            .iter()
            .map(|b| decompress(b).map_or(0, |v| v.len()))
            .sum();
        eprintln!(
            "decompress 2000: {:.1} ms ({n})",
            t.elapsed().as_secs_f64() * 1e3
        );
    }

    #[test]
    fn weekly_rings_line_up() {
        const WEEK: i64 = 7 * 24 * 3600 * 1000;
        let w = merge_weeks(&[2, 1], Some(10 * WEEK), &[3], Some(12 * WEEK));
        assert_eq!(&w[..4], &[3, 0, 2, 1]);
    }
}
