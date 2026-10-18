//! Storage side of the playlist continuation (T3.14, FR-PLS-003/005, docs/04 §11): the sound
//! data of every playable track, what must never be offered, ∞ per playlist and «−».

use std::collections::HashSet;

use rusqlite::{Connection, params};
use sirtone_core::wave::SimTrack;

use crate::db::now_ms;
use crate::error::LibraryError;
use crate::playlists::PlaylistId;
use crate::repo::TrackId;

/// Every playable track with what its sound vector is built from.
pub fn sim_tracks(conn: &Connection) -> Result<Vec<SimTrack>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT t.id,
                coalesce((SELECT ta.artist_id FROM track_artist ta WHERE ta.track_id = t.id
                          ORDER BY ta.position LIMIT 1), -t.id),
                t.play_count, t.bpm, t.key_camelot, t.valence, t.arousal, t.energy, t.embedding
         FROM track t WHERE t.state = 'ok'",
    )?;
    let rows = st
        .query_map([], |r| {
            let vae = match (
                r.get::<_, Option<f64>>(5)?,
                r.get::<_, Option<f64>>(6)?,
                r.get::<_, Option<f64>>(7)?,
            ) {
                (Some(v), Some(a), Some(e)) => Some([v as f32, a as f32, e as f32]),
                _ => None,
            };
            Ok(SimTrack {
                id: r.get(0)?,
                artist_id: r.get(1)?,
                play_count: r.get(2)?,
                bpm: r.get::<_, Option<f64>>(3)?.map(|v| v as f32),
                key_camelot: r.get(4)?,
                vae,
                embedding: r.get(8)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// What the continuation must not offer: «никогда» of the track or its primary artist (Q-14),
/// tracks played since `recent_ms`, and «−» of this playlist.
pub fn blocked(
    conn: &Connection,
    playlist: Option<PlaylistId>,
    recent_ms: i64,
) -> Result<HashSet<i64>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT t.id FROM track t WHERE t.wave_never = 1 OR coalesce((SELECT a.wave_never
                FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                WHERE ta.track_id = t.id ORDER BY ta.position LIMIT 1), 0) = 1
         UNION SELECT track_id FROM play_event WHERE ts >= ?1
         UNION SELECT track_id FROM playlist_exclude WHERE playlist_id = ?2",
    )?;
    let ids = st
        .query_map(params![recent_ms, playlist.unwrap_or(-1)], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

/// ∞ of a playlist: manual playlists only (a smart one is its rules).
pub fn hybrid(conn: &Connection, playlist: PlaylistId) -> Result<bool, LibraryError> {
    Ok(conn
        .query_row(
            "SELECT hybrid = 1 AND kind = 'manual' FROM playlist WHERE id = ?1",
            [playlist],
            |r| r.get(0),
        )
        .unwrap_or(false))
}

pub fn set_hybrid(conn: &Connection, playlist: PlaylistId, on: bool) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE playlist SET hybrid = ?2 WHERE id = ?1 AND kind = 'manual'",
        params![playlist, on],
    )?;
    Ok(())
}

/// «−» on a recommended track: not offered for this playlist again.
pub fn exclude(
    conn: &Connection,
    playlist: PlaylistId,
    track: TrackId,
) -> Result<(), LibraryError> {
    conn.execute(
        "INSERT OR IGNORE INTO playlist_exclude(playlist_id, track_id, created_at)
         VALUES (?1, ?2, ?3)",
        params![playlist, track, now_ms()],
    )?;
    Ok(())
}

/// The playlist's own tracks in order (the seeds of its continuation).
pub fn seeds(conn: &Connection, playlist: PlaylistId) -> Result<Vec<i64>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT i.track_id FROM playlist_item i JOIN track t ON t.id = i.track_id
         WHERE i.playlist_id = ?1 AND t.state = 'ok' ORDER BY i.position",
    )?;
    let ids = st
        .query_map([playlist], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open;
    use crate::repo::{Identity, NewTrack, artist_get_or_create, track_insert};

    fn lib() -> Connection {
        let root = std::env::temp_dir().join(format!("sirtone-cont-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap()
    }

    fn track(c: &mut Connection, n: u32, artist: &str) -> TrackId {
        let a = artist_get_or_create(c, artist).unwrap();
        track_insert(
            c,
            &NewTrack {
                rel_path: format!("A/{n}.mp3"),
                size_bytes: 1,
                mtime_ms: 0,
                content_hash: format!("h{n}"),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: 200_000,
                title: format!("t{n}"),
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

    #[test]
    fn blocked_hybrid_and_exclusions() {
        let mut c = lib();
        let t: Vec<TrackId> = (1..=5)
            .map(|n| track(&mut c, n, if n == 4 { "B" } else { "A" }))
            .collect();
        let pl = crate::playlists::create(&c, "Дорога").unwrap();
        crate::playlists::add(&mut c, pl, &t[..2]).unwrap();
        assert_eq!(seeds(&c, pl).unwrap(), t[..2].to_vec());
        // New manual playlists continue by default (FR-PLS-005); the switch sticks.
        assert!(hybrid(&c, pl).unwrap());
        set_hybrid(&c, pl, false).unwrap();
        assert!(!hybrid(&c, pl).unwrap());
        c.execute("UPDATE track SET wave_never = 1 WHERE id = ?1", [t[2]])
            .unwrap();
        c.execute("UPDATE artist SET wave_never = 1 WHERE name = 'B'", [])
            .unwrap();
        exclude(&c, pl, t[4]).unwrap();
        exclude(&c, pl, t[4]).unwrap();
        let b = blocked(&c, Some(pl), 0).unwrap();
        assert_eq!(b, [t[2], t[3], t[4]].into());
        // Another playlist's «−» does not count.
        assert_eq!(blocked(&c, None, 0).unwrap(), [t[2], t[3]].into());
        assert_eq!(sim_tracks(&c).unwrap().len(), 5);
    }
}
