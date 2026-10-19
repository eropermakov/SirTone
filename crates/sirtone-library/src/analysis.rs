//! Storage of analysis results (T3.6/T3.10, T3.8/T3.9): the queue of tracks still to analyse,
//! the B0/B1 and vibe columns of `track` and the wave vector in `track_features` (docs/03 §7,
//! docs/14 §4). Files are only read, never changed.

use rusqlite::{Connection, params};
use sirtone_core::analysis::B0;

use crate::db::now_ms;
use crate::error::LibraryError;
use crate::repo::TrackId;

/// Playable tracks whose analysis is older than `version`, newest first: id, path and the
/// duration from the tags (it places the tagger windows).
pub fn pending(
    conn: &Connection,
    version: i64,
    limit: u32,
) -> Result<Vec<(TrackId, String, u64)>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT id, rel_path, duration_ms FROM track WHERE state = 'ok' AND analysis_version < ?1
         ORDER BY added_at DESC, id DESC LIMIT ?2",
    )?;
    let rows = st
        .query_map(params![version, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)?.max(0) as u64))
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// How many tracks are analysed / in total (progress in Settings).
pub fn progress(conn: &Connection, version: i64) -> Result<(u32, u32), LibraryError> {
    Ok(conn.query_row(
        "SELECT coalesce(sum(analysis_version >= ?1), 0), count(*) FROM track WHERE state = 'ok'",
        [version],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

pub fn store_b0(
    conn: &Connection,
    track: TrackId,
    b: &B0,
    version: i64,
) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE track SET lufs_i = ?2, true_peak_db = ?3, lra = ?4, peaks = ?5,
                lead_silence_ms = ?6, tail_silence_ms = ?7, env_head = ?8, env_tail = ?9,
                analysis_version = ?10, analyzed_at = ?11, fingerprint = ?12, fp_duration_ms = ?13,
                fp_sketch = ?14, bpm = ?15, bpm_conf = ?16, key_camelot = ?17, key_conf = ?18
         WHERE id = ?1",
        params![
            track,
            b.lufs_i,
            b.true_peak_db,
            b.lra,
            b.peaks,
            b.lead_silence_ms as i64,
            b.tail_silence_ms as i64,
            b.env_head,
            b.env_tail,
            version,
            now_ms(),
            b.fingerprint,
            b.fingerprint.as_ref().map(|_| b.fp_duration_ms as i64),
            b.fp_sketch,
            b.bpm,
            b.bpm_conf,
            b.key_camelot,
            b.key_conf,
        ],
    )?;
    if let Some(v) = &b.vibe {
        // `lang` is only ever set to «без слов» here; a language found otherwise stays.
        conn.execute(
            "UPDATE track SET valence = ?2, arousal = ?3, aggression = ?4, energy = ?5,
                    danceability = ?6, acousticness = ?7, vocal = ?8, embedding = ?9,
                    lang = CASE WHEN ?10 THEN 'none' WHEN lang = 'none' THEN NULL ELSE lang END
             WHERE id = ?1",
            params![
                track,
                v.valence,
                v.arousal,
                v.aggression,
                v.energy,
                v.danceability,
                v.acousticness,
                v.vocal,
                b.embedding,
                v.instrumental,
            ],
        )?;
    }
    Ok(())
}

/// The wave vector (`docs/14` §4.1, f32 LE) with its completeness and the SQL filter columns
/// derived from it (`mood_bits`, `activity_json`). Seasonal affinity and keyword bits of the
/// row are kept.
pub fn store_features(
    conn: &Connection,
    track: TrackId,
    vec: &[f32],
    schema: i64,
    conf: f32,
    mood_bits: u8,
    activity_json: &str,
) -> Result<(), LibraryError> {
    let blob: Vec<u8> = vec.iter().flat_map(|v| v.to_le_bytes()).collect();
    conn.execute(
        "INSERT INTO track_features(track_id, version, vec, conf, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(track_id) DO UPDATE SET version = excluded.version, vec = excluded.vec,
             conf = excluded.conf, updated_at = excluded.updated_at",
        params![track, schema, blob, f64::from(conf), now_ms()],
    )?;
    conn.execute(
        "UPDATE track SET mood_bits = ?2, activity_json = ?3 WHERE id = ?1",
        params![track, i64::from(mood_bits), activity_json],
    )?;
    Ok(())
}

/// A file that cannot be analysed is not retried at this version (it stays playable).
pub fn mark_failed(conn: &Connection, track: TrackId, version: i64) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE track SET analysis_version = ?2, analyzed_at = ?3 WHERE id = ?1",
        params![track, version, now_ms()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open;
    use crate::repo::{Identity, NewTrack, artist_get_or_create, track_insert};
    use sirtone_core::analysis::Vibe;

    fn lib() -> Connection {
        let root = std::env::temp_dir().join(format!("sirtone-an-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap()
    }

    fn track(c: &mut Connection) -> TrackId {
        let a = artist_get_or_create(c, "A").unwrap();
        track_insert(
            c,
            &NewTrack {
                rel_path: "A/1.mp3".into(),
                size_bytes: 1,
                mtime_ms: 0,
                content_hash: "h".into(),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: 200_000,
                title: "t".into(),
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
        .unwrap()
    }

    fn b0(vibe: Option<Vibe>) -> B0 {
        B0 {
            duration_ms: 200_000,
            lufs_i: Some(-9.0),
            true_peak_db: None,
            lra: None,
            peaks: vec![0; 400],
            lead_silence_ms: 0,
            tail_silence_ms: 0,
            env_head: vec![],
            env_tail: vec![],
            fingerprint: None,
            fp_duration_ms: 0,
            fp_sketch: None,
            bpm: Some(120.0),
            bpm_conf: None,
            key_camelot: None,
            key_conf: None,
            embedding: vibe.as_ref().map(|_| vec![1; 68]),
            vibe,
        }
    }

    fn vibe(vocal: f32) -> Vibe {
        Vibe {
            energy: 0.8,
            valence: 0.7,
            arousal: 0.6,
            danceability: 0.5,
            acousticness: 0.1,
            vocal,
            aggression: 0.2,
            brightness: 0.5,
            moods: [0.0; 8],
            genres: [0.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            instrumental: vocal < 0.2,
            conf: 1.0,
        }
    }

    #[test]
    fn vibe_columns_language_and_features() {
        let mut c = lib();
        let id = track(&mut c);
        let pending = super::pending(&c, 4, 10).unwrap();
        assert_eq!(pending, vec![(id, "A/1.mp3".to_string(), 200_000)]);
        store_b0(&c, id, &b0(Some(vibe(0.1))), 4).unwrap();
        let row: (f64, String, i64) = c
            .query_row(
                "SELECT vocal, lang, length(embedding) FROM track WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert!((row.0 - 0.1).abs() < 1e-6);
        assert_eq!((row.1.as_str(), row.2), ("none", 68));
        // With singing the «без слов» we set is withdrawn; a language found elsewhere stays.
        store_b0(&c, id, &b0(Some(vibe(0.8))), 4).unwrap();
        let lang: Option<String> = c
            .query_row("SELECT lang FROM track WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(lang, None);
        c.execute("UPDATE track SET lang = 'ru' WHERE id = ?1", [id])
            .unwrap();
        store_b0(&c, id, &b0(None), 4).unwrap();
        store_b0(&c, id, &b0(Some(vibe(0.8))), 4).unwrap();
        let lang: String = c
            .query_row("SELECT lang FROM track WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        assert_eq!(lang, "ru");
        // The vector is upserted; the seasonal affinity of the row survives.
        store_features(&c, id, &[0.5; 32], 1, 1.0, 0b101, r#"{"party":0.9}"#).unwrap();
        c.execute(
            "UPDATE track_features SET season_aff = x'01', kw_bits = 3 WHERE track_id = ?1",
            [id],
        )
        .unwrap();
        store_features(&c, id, &[0.25; 32], 1, 0.8, 0b10, r#"{"party":0.1}"#).unwrap();
        let (len, conf, aff, kw, bits): (i64, f64, Vec<u8>, i64, i64) = c
            .query_row(
                "SELECT length(tf.vec), tf.conf, tf.season_aff, tf.kw_bits, t.mood_bits
                 FROM track_features tf JOIN track t ON t.id = tf.track_id WHERE tf.track_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!((len, aff, kw, bits), (128, vec![1], 3, 0b10));
        assert!((conf - 0.8).abs() < 1e-6);
    }
}
