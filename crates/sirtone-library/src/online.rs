//! Storage side of the online add-on (2.0, D-84, D-87; `05` §4.6–4.7, FR-REC-006): which
//! «Неизвестные» can be looked up by text, and what a lookup leaves behind — a confirmed
//! identity (the move itself goes through `relocate::assign`) or hints.

use rusqlite::{Connection, params};

use crate::error::LibraryError;
use crate::repo::TrackId;

/// An unknown track worth a text lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolvable {
    pub id: TrackId,
    pub rel_path: String,
    pub title: String,
    /// Primary artist, if the library has one for it.
    pub artist: Option<String>,
    /// All its artists as the track card shows them (", "-joined).
    pub artists_joined: String,
    pub album: Option<String>,
    pub duration_ms: u64,
    pub reason: Option<String>,
}

/// Unknown tracks after `after` (by id), oldest first; long files («возможно, не трек») are not
/// looked up.
pub fn resolvable(
    conn: &Connection,
    after: TrackId,
    limit: u32,
) -> Result<Vec<Resolvable>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT t.id, t.rel_path, t.title,
                (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                 WHERE ta.track_id = t.id ORDER BY ta.position LIMIT 1),
                al.title, t.duration_ms, t.unknown_reason,
                coalesce((SELECT group_concat(name, ', ') FROM
                  (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                   WHERE ta.track_id = t.id ORDER BY ta.position)), '')
         FROM track t LEFT JOIN album al ON al.id = t.album_id
         WHERE t.state = 'ok' AND t.identity_state = 'unknown' AND t.id > ?1
           AND coalesce(t.unknown_reason, '') <> 'possible_non_track' AND t.user_normal = 0
         ORDER BY t.id LIMIT ?2",
    )?;
    let rows = st
        .query_map(params![after, limit], |r| {
            Ok(Resolvable {
                id: r.get(0)?,
                rel_path: r.get(1)?,
                title: r.get(2)?,
                artist: r.get(3)?,
                album: r.get(4)?,
                duration_ms: r.get::<_, i64>(5)?.max(0) as u64,
                reason: r.get(6)?,
                artists_joined: r.get(7)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// After the move: the identity is confirmed by MusicBrainz.
pub fn confirm(
    conn: &Connection,
    track: TrackId,
    mbid: &str,
    confidence: f64,
) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE track SET identity_state = 'confirmed', identity_conf = ?2, mbid_recording = ?3,
                unknown_reason = NULL, candidates_json = NULL WHERE id = ?1",
        params![track, confidence.clamp(0.0, 1.0), mbid],
    )?;
    Ok(())
}

/// Hints for «Неизвестные» (`None` clears them); an unchanged value is not written again.
pub fn set_candidates(
    conn: &Connection,
    track: TrackId,
    json: Option<&str>,
) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE track SET candidates_json = ?2
         WHERE id = ?1 AND identity_state = 'unknown' AND candidates_json IS NOT ?2",
        params![track, json],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::repo::{Identity, NewTrack, artist_get_or_create, track_insert};

    fn track(c: &mut Connection, n: u32, identity: Identity, dur: u64) -> TrackId {
        let a = artist_get_or_create(c, "Кино").unwrap();
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
                duration_ms: dur,
                title: "Группа крови".into(),
                album_id: None,
                disc_no: None,
                track_no: None,
                year: None,
                identity,
                identity_conf: 0.1,
                unknown_reason: matches!(identity, Identity::Unknown)
                    .then(|| "no_metadata".to_string()),
                artists: vec![a],
            },
        )
        .unwrap()
    }

    #[test]
    fn resolvable_confirm_and_hints() {
        let mut c = open_in_memory().unwrap();
        let a = track(&mut c, 1, Identity::Unknown, 286_000);
        let _known = track(&mut c, 2, Identity::Inferred, 286_000);
        let long = track(&mut c, 3, Identity::Unknown, 3_600_000);
        c.execute(
            "UPDATE track SET unknown_reason = 'possible_non_track' WHERE id = ?1",
            [long],
        )
        .unwrap();
        let r = resolvable(&c, 0, 10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, a);
        assert_eq!(r[0].artist.as_deref(), Some("Кино"));
        assert!(resolvable(&c, a, 10).unwrap().is_empty());
        set_candidates(&c, a, Some("[]")).unwrap();
        confirm(&c, a, "mbid-1", 0.97).unwrap();
        let (state, mbid, cand): (String, String, Option<String>) = c
            .query_row(
                "SELECT identity_state, mbid_recording, candidates_json FROM track WHERE id = ?1",
                [a],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (state.as_str(), mbid.as_str(), cand),
            ("confirmed", "mbid-1", None)
        );
    }
}
