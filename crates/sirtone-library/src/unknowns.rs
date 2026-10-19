//! «Неизвестные» II (T3.11, FR-REC-008, FR-IMP-007/008, `05` §4.7; Picard «Unmatched»,
//! iTunes «Not Added», `01b` F-08): hints in one click, «нормальный» for long files, and the
//! quarantine for files that stopped playing — never a silent delete: the user decides
//! (Recycle Bin or back).

use rusqlite::{Connection, params};
use serde_json::Value;

use crate::collection::{TRACK_COLS, track_row};
use crate::db::now_ms;
use crate::error::LibraryError;
use crate::repo::{TrackId, TrackRow};

/// A hint for an unknown track (`candidates_json`, written by the online lookup, D-87).
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub title: String,
    pub artists: Vec<String>,
    pub mbid: Option<String>,
    /// Weighted distance (`05` §4.4): 0 — the same.
    pub d: f64,
    pub source: String,
}

/// The hints of a track, best first.
pub fn candidates(conn: &Connection, track: TrackId) -> Result<Vec<Candidate>, LibraryError> {
    let json: Option<String> = conn
        .query_row(
            "SELECT candidates_json FROM track WHERE id = ?1",
            [track],
            |r| r.get(0),
        )
        .unwrap_or(None);
    Ok(parse(json.as_deref()))
}

/// `candidates_json` → hints (unknown shapes are skipped).
pub fn parse(json: Option<&str>) -> Vec<Candidate> {
    let Some(v) = json.and_then(|j| serde_json::from_str::<Value>(j).ok()) else {
        return Vec::new();
    };
    let mut out: Vec<Candidate> = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let title = c["title"].as_str()?.trim().to_string();
            let artists: Vec<String> = c["artists"]
                .as_array()?
                .iter()
                .filter_map(|a| a.as_str().map(str::to_string))
                .collect();
            (!title.is_empty() && !artists.is_empty()).then(|| Candidate {
                title,
                artists,
                mbid: c["mbid"]
                    .as_str()
                    .filter(|m| sirtone_core::ports::valid_mbid(m))
                    .map(str::to_string),
                d: c["d"].as_f64().unwrap_or(1.0),
                source: c["source"].as_str().unwrap_or("").to_string(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.d.total_cmp(&b.d));
    out
}

/// «Нормальный» (FR-IMP-007): a long file is music after all — it stays in «Неизвестные» as an
/// ordinary unknown, to be assigned (artist, album, playlist) like any other.
pub fn mark_normal(conn: &Connection, tracks: &[TrackId]) -> Result<u32, LibraryError> {
    let mut st = conn.prepare_cached(
        "UPDATE track SET user_normal = 1,
                unknown_reason = CASE unknown_reason WHEN 'possible_non_track' THEN 'needs_review'
                                 ELSE unknown_reason END
         WHERE id = ?1",
    )?;
    let mut n = 0;
    for t in tracks {
        n += st.execute([t])? as u32;
    }
    Ok(n)
}

/// FR-IMP-008: a library file that stopped playing (the decoder refuses it) leaves the lists —
/// the file stays where it is until the user decides.
pub fn quarantine(conn: &Connection, track: TrackId) -> Result<bool, LibraryError> {
    Ok(conn.execute(
        "UPDATE track SET state = 'quarantined', analyzed_at = ?2 WHERE id = ?1 AND state = 'ok'",
        params![track, now_ms()],
    )? > 0)
}

/// «Вернуть»: the track is back and analysed again (a file fixed outside, a false alarm).
pub fn release(conn: &Connection, track: TrackId) -> Result<bool, LibraryError> {
    Ok(conn.execute(
        "UPDATE track SET state = 'ok', analysis_version = 0 WHERE id = ?1 AND state = 'quarantined'",
        [track],
    )? > 0)
}

/// Tracks in quarantine, newest first.
pub fn quarantined(conn: &Connection, limit: u32) -> Result<Vec<TrackRow>, LibraryError> {
    let mut st = conn.prepare_cached(&format!(
        "SELECT {TRACK_COLS} FROM track t LEFT JOIN album al ON al.id = t.album_id
         WHERE t.state = 'quarantined' ORDER BY t.analyzed_at DESC, t.id DESC LIMIT ?1"
    ))?;
    let rows = st
        .query_map([limit], track_row)?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::repo::{Identity, NewTrack, artist_get_or_create, track_insert};

    fn track(c: &mut Connection, n: u32, reason: &str) -> TrackId {
        let a = artist_get_or_create(c, "A").unwrap();
        track_insert(
            c,
            &NewTrack {
                rel_path: format!("_Unknown/{n}.mp3"),
                size_bytes: 1,
                mtime_ms: 0,
                content_hash: format!("h{n}"),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: 10_800_000,
                title: format!("mix {n}"),
                album_id: None,
                disc_no: None,
                track_no: None,
                year: None,
                identity: Identity::Unknown,
                identity_conf: 0.1,
                unknown_reason: Some(reason.into()),
                artists: vec![a],
            },
        )
        .unwrap()
    }

    #[test]
    fn hints_normal_and_quarantine() {
        assert_eq!(
            parse(Some(
                r#"[{"source":"musicbrainz","title":"B","artists":["X"],"d":0.2},
                    {"title":"A","artists":["Y"],"mbid":"f00dbabe-0000-4000-8000-000000000001","d":0.1},
                    {"title":"","artists":["Z"]}, 5]"#
            ))
            .iter()
            .map(|c| (c.title.as_str(), c.mbid.is_some()))
            .collect::<Vec<_>>(),
            vec![("A", true), ("B", false)]
        );
        assert!(parse(Some("not json")).is_empty());
        let mut c = open_in_memory().unwrap();
        let long = track(&mut c, 1, "possible_non_track");
        let other = track(&mut c, 2, "no_metadata");
        assert_eq!(mark_normal(&c, &[long, other]).unwrap(), 2);
        let (reason, normal): (String, i64) = c
            .query_row(
                "SELECT unknown_reason, user_normal FROM track WHERE id = ?1",
                [long],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((reason.as_str(), normal), ("needs_review", 1));
        assert!(quarantine(&c, other).unwrap());
        assert!(!quarantine(&c, other).unwrap());
        assert_eq!(quarantined(&c, 10).unwrap().len(), 1);
        assert!(release(&c, other).unwrap());
        assert!(quarantined(&c, 10).unwrap().is_empty());
    }
}
