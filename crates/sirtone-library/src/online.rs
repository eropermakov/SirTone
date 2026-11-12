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
    release_group: Option<&str>,
) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE track SET identity_state = 'confirmed', identity_conf = ?2, mbid_recording = ?3,
                unknown_reason = NULL, candidates_json = NULL WHERE id = ?1",
        params![track, confidence.clamp(0.0, 1.0), mbid],
    )?;
    // The album learns its release group (a cover from the Cover Art Archive, T3.3).
    if let Some(rg) = release_group.filter(|g| sirtone_core::ports::valid_mbid(g)) {
        conn.execute(
            "UPDATE album SET mbid_group = coalesce(mbid_group, ?2)
             WHERE id = (SELECT album_id FROM track WHERE id = ?1)",
            params![track, rg],
        )?;
    }
    Ok(())
}

/// An album waiting for a Cover Art Archive cover: id, release, release group.
pub type CaaAlbum = (i64, Option<String>, Option<String>);

/// Albums without a cover (and not locked by the user) that know a release or release group.
pub fn albums_for_caa(conn: &Connection, limit: u32) -> Result<Vec<CaaAlbum>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT id, mbid_release, mbid_group FROM album
         WHERE cover_id IS NULL AND cover_locked = 0
           AND (mbid_release IS NOT NULL OR mbid_group IS NOT NULL)
         ORDER BY id LIMIT ?1",
    )?;
    let rows = st
        .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Albums without a cover (not locked by the user): id, title, album artist — for the web search
/// (D-162). `limit` per run.
pub fn albums_for_web_cover(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<(i64, String, String)>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT a.id, a.title, ar.name FROM album a JOIN artist ar ON ar.id = a.album_artist_id
         WHERE a.cover_id IS NULL AND a.cover_locked = 0 AND a.kind != 'unknown'
         ORDER BY a.id LIMIT ?1",
    )?;
    let rows = st
        .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Artists with playable tracks but no album cover and no photo yet: id, name (D-173).
pub fn artists_for_photo(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<(i64, String)>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT a.id, a.name FROM artist a
         WHERE a.merged_into IS NULL AND a.photo_cover_id IS NULL
           AND EXISTS (SELECT 1 FROM track_artist ta JOIN track t ON t.id = ta.track_id
                       WHERE ta.artist_id = a.id AND t.state = 'ok')
           AND NOT EXISTS (SELECT 1 FROM track_artist ta JOIN track t ON t.id = ta.track_id
                           JOIN album al ON al.id = t.album_id
                           WHERE ta.artist_id = a.id AND t.state = 'ok' AND al.cover_id IS NOT NULL)
         ORDER BY a.id LIMIT ?1",
    )?;
    let rows = st
        .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// A track still without any cover (D-173): id, title, first artist id and name.
pub type WebTrack = (i64, String, i64, String);

/// Playable tracks whose own cover and album cover are both missing (no album, an «unknown» album
/// or an album without a cover that the user did not lock).
pub fn tracks_for_web_cover(conn: &Connection, limit: u32) -> Result<Vec<WebTrack>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT t.id, t.title, ta.artist_id, ar.name
         FROM track t
         LEFT JOIN album al ON al.id = t.album_id
         JOIN track_artist ta ON ta.track_id = t.id
           AND ta.position = (SELECT min(position) FROM track_artist WHERE track_id = t.id)
         JOIN artist ar ON ar.id = ta.artist_id
         WHERE t.state = 'ok' AND t.cover_id IS NULL AND t.identity_state != 'unknown'
           AND (al.id IS NULL OR (al.cover_id IS NULL AND al.cover_locked = 0))
         ORDER BY t.id LIMIT ?1",
    )?;
    let rows = st
        .query_map([limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
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
    fn web_photo_and_track_cover_candidates() {
        let mut c = open_in_memory().unwrap();
        let t = track(&mut c, 1, Identity::Inferred, 200_000);
        let artist: i64 = c
            .query_row(
                "SELECT artist_id FROM track_artist WHERE track_id = ?1",
                [t],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            artists_for_photo(&c, 10).unwrap(),
            vec![(artist, "Кино".to_string())]
        );
        let w = tracks_for_web_cover(&c, 10).unwrap();
        assert_eq!(
            w,
            vec![(t, "Группа крови".to_string(), artist, "Кино".to_string())]
        );

        c.execute(
            "INSERT INTO cover(id, hash, source, created_at) VALUES (1, 'hh', 'caa', 0)",
            [],
        )
        .unwrap();
        crate::covers::link_artist(&c, artist, 1).unwrap();
        assert!(artists_for_photo(&c, 10).unwrap().is_empty());
        assert_eq!(crate::covers::artist_photo(&c, artist).unwrap(), Some(1));
        let covers = crate::collection::artist_covers(&c, &[artist]).unwrap();
        assert_eq!(
            covers.get(&artist).map(String::as_str),
            Some("hh"),
            "photo fills the card"
        );

        crate::covers::link_track(&c, t, 1).unwrap();
        assert!(tracks_for_web_cover(&c, 10).unwrap().is_empty());
        // A cover of the track stays: a later one does not replace it.
        c.execute(
            "INSERT INTO cover(id, hash, source, created_at) VALUES (2, 'h2', 'caa', 0)",
            [],
        )
        .unwrap();
        crate::covers::link_track(&c, t, 2).unwrap();
        let kept: i64 = c
            .query_row("SELECT cover_id FROM track WHERE id = ?1", [t], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kept, 1);
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
        confirm(&c, a, "mbid-1", 0.97, None).unwrap();
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
