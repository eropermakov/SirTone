//! Popularity for «Для всех» (T3.32, FR-ALL-002, `14` §16, `01c` §12): ListenBrainz
//! `POST /1/popularity/recording` by recording MBID (tags written by Picard, or confirmed online,
//! D-87) → `popularity_cache`, refreshed once a month. Unique listeners count, not plays.

use rusqlite::{Connection, params};
use serde_json::Value;

use crate::error::LibraryError;

/// A month: the cache is refreshed after it (FR-ALL-002).
pub const REFRESH_MS: i64 = 30 * 24 * 3600 * 1000;
/// MBIDs per request.
pub const BATCH: usize = 100;

/// Recording MBIDs of playable tracks whose popularity is missing or older than `before_ms`.
pub fn stale(conn: &Connection, before_ms: i64, limit: u32) -> Result<Vec<String>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT DISTINCT t.mbid_recording FROM track t
         LEFT JOIN popularity_cache pc ON pc.mbid = t.mbid_recording
         WHERE t.state = 'ok' AND t.mbid_recording IS NOT NULL
           AND (pc.mbid IS NULL OR pc.fetched_at < ?1)
         LIMIT ?2",
    )?;
    let rows = st
        .query_map(params![before_ms, limit], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// The request body for a batch.
pub fn request(mbids: &[String]) -> Vec<u8> {
    serde_json::json!({ "recording_mbids": mbids })
        .to_string()
        .into_bytes()
}

/// Stores an answer (`[{recording_mbid, total_listen_count, total_user_count}]`, nulls for
/// recordings without data); returns how many rows were written.
pub fn store(conn: &Connection, answer: &[u8], now_ms: i64) -> Result<usize, LibraryError> {
    let v: Value = serde_json::from_slice(answer)
        .map_err(|e| LibraryError::Invalid(format!("popularity: {e}")))?;
    let mut st = conn.prepare_cached(
        "INSERT INTO popularity_cache(mbid, listeners, listens, fetched_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(mbid) DO UPDATE SET listeners = excluded.listeners,
             listens = excluded.listens, fetched_at = excluded.fetched_at",
    )?;
    let mut n = 0;
    for r in v.as_array().into_iter().flatten() {
        let Some(mbid) = r["recording_mbid"]
            .as_str()
            .filter(|m| sirtone_core::ports::valid_mbid(m))
        else {
            continue;
        };
        st.execute(params![
            mbid,
            r["total_user_count"].as_i64(),
            r["total_listen_count"].as_i64(),
            now_ms
        ])?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn stale_request_store() {
        let c = open_in_memory().unwrap();
        c.execute_batch(
            "INSERT INTO track(uid, rel_path, size_bytes, mtime_ms, content_hash, container, codec,
               lossless, duration_ms, title, title_norm, identity_state, added_at, mbid_recording)
             VALUES ('u1', 'a.mp3', 1, 0, 'h1', 'mp3', 'mp3', 0, 1, 'A', 'a', 'confirmed', 0,
                     'f00dbabe-0000-4000-8000-000000000001'),
                    ('u2', 'b.mp3', 1, 0, 'h2', 'mp3', 'mp3', 0, 1, 'B', 'b', 'confirmed', 0, NULL);",
        )
        .unwrap();
        let s = stale(&c, 1_000, 10).unwrap();
        assert_eq!(s, vec!["f00dbabe-0000-4000-8000-000000000001".to_string()]);
        assert_eq!(
            String::from_utf8(request(&s)).unwrap(),
            r#"{"recording_mbids":["f00dbabe-0000-4000-8000-000000000001"]}"#
        );
        let answer = br#"[{"recording_mbid":"f00dbabe-0000-4000-8000-000000000001",
            "total_listen_count":5000,"total_user_count":1200},
            {"recording_mbid":"../bad","total_user_count":1}]"#;
        assert_eq!(store(&c, answer, 2_000).unwrap(), 1);
        // Fresh now; stale again a month later.
        assert!(stale(&c, 1_500, 10).unwrap().is_empty());
        assert_eq!(stale(&c, 2_000 + REFRESH_MS + 1, 10).unwrap().len(), 1);
        let l: i64 = c
            .query_row("SELECT listeners FROM popularity_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(l, 1200);
        // A recording without data is stored with nulls (asked again next month, not now).
        store(
            &c,
            br#"[{"recording_mbid":"f00dbabe-0000-4000-8000-000000000001","total_listen_count":null,"total_user_count":null}]"#,
            3_000,
        )
        .unwrap();
        assert!(stale(&c, 2_500, 10).unwrap().is_empty());
        // The wave reads it by the track's MBID (null now → unknown).
        let cat = crate::wave::catalog(&c).unwrap();
        assert!(cat.iter().all(|t| t.listeners.is_none()));
        store(&c, answer, 4_000).unwrap();
        let cat = crate::wave::catalog(&c).unwrap();
        assert_eq!(
            cat.iter().filter_map(|t| t.listeners).collect::<Vec<_>>(),
            vec![1200]
        );
    }
}
