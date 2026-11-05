//! What the transfer package (PC ↔ phone, D-140, M6.2) reads from a library. Plain rows only:
//! the composition root maps them onto `sirtone-transfer` (domain crates do not depend on each
//! other).

use rusqlite::{Connection, OptionalExtension};

use crate::LibraryError;

/// The library's own id (`meta.library_uid`): prefix of every key in a package.
pub fn library_uid(conn: &Connection) -> Result<String, LibraryError> {
    Ok(conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'library_uid'",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_default())
}

/// A track as the package needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferTrack {
    pub id: i64,
    pub rel_path: String,
    pub size: u64,
    pub duration_ms: u32,
    pub artist: Option<String>,
    pub title: String,
    pub album: Option<String>,
    /// Compressed Chromaprint as stored (`sirtone_core::fingerprint::decompress`).
    pub fingerprint: Option<Vec<u8>>,
}

/// Every track of the library, in id order.
pub fn tracks(conn: &Connection) -> Result<Vec<TransferTrack>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT t.id, t.rel_path, t.size_bytes, t.duration_ms, t.title, al.title,
                (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'primary' ORDER BY ta.position LIMIT 1),
                t.fingerprint
           FROM track t LEFT JOIN album al ON al.id = t.album_id
          ORDER BY t.id",
    )?;
    let rows = st.query_map([], |r| {
        Ok(TransferTrack {
            id: r.get(0)?,
            rel_path: r.get(1)?,
            size: r.get::<_, i64>(2)?.max(0) as u64,
            duration_ms: r.get::<_, i64>(3)?.clamp(0, i64::from(u32::MAX)) as u32,
            title: r.get(4)?,
            album: r.get(5)?,
            artist: r.get(6)?,
            fingerprint: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// One journal line for the package: a play (`play_event`) or a track mark (`feedback`).
#[derive(Debug, Clone, PartialEq)]
pub enum JournalRow {
    Play {
        id: i64,
        ts: i64,
        track_id: i64,
        played_ms: i64,
        duration_ms: i64,
        end_reason: String,
        source: String,
    },
    Mark {
        id: i64,
        ts: i64,
        track_id: i64,
        kind: String,
    },
}

/// The whole journal: plays and track marks (artist marks stay on the device), oldest first.
pub fn journal(conn: &Connection) -> Result<Vec<JournalRow>, LibraryError> {
    let mut out = Vec::new();
    let mut st = conn.prepare(
        "SELECT id, ts, track_id, played_ms, duration_ms, end_reason, source
           FROM play_event ORDER BY ts, id",
    )?;
    for row in st.query_map([], |r| {
        Ok(JournalRow::Play {
            id: r.get(0)?,
            ts: r.get(1)?,
            track_id: r.get(2)?,
            played_ms: r.get(3)?,
            duration_ms: r.get(4)?,
            end_reason: r.get(5)?,
            source: r.get(6)?,
        })
    })? {
        out.push(row?);
    }
    let mut st = conn.prepare(
        "SELECT id, ts, track_id, kind FROM feedback
          WHERE track_id IS NOT NULL ORDER BY ts, id",
    )?;
    for row in st.query_map([], |r| {
        Ok(JournalRow::Mark {
            id: r.get(0)?,
            ts: r.get(1)?,
            track_id: r.get(2)?,
            kind: r.get(3)?,
        })
    })? {
        out.push(row?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::repo::{
        Identity, NewTrack, album_get_or_create, artist_get_or_create, track_insert,
    };

    #[test]
    fn tracks_and_journal_for_a_package() {
        let mut c = open_in_memory().unwrap();
        let kino = artist_get_or_create(&c, "Кино").unwrap();
        let album = album_get_or_create(&c, "Группа крови", kino, Some(1988)).unwrap();
        let id = track_insert(
            &mut c,
            &NewTrack {
                rel_path: "Кино/Группа крови/01 Группа крови.flac".into(),
                size_bytes: 30_000_000,
                mtime_ms: 0,
                content_hash: "h".into(),
                container: "flac".into(),
                codec: "flac".into(),
                lossless: true,
                sample_rate: Some(44100),
                channels: Some(2),
                duration_ms: 287_000,
                title: "Группа крови".into(),
                album_id: Some(album),
                disc_no: None,
                track_no: Some(1),
                year: Some(1988),
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![kino],
            },
        )
        .unwrap();
        assert!(!library_uid(&c).unwrap().is_empty());
        let t = tracks(&c).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].artist.as_deref(), Some("Кино"));
        assert_eq!(t[0].album.as_deref(), Some("Группа крови"));
        assert_eq!((t[0].size, t[0].duration_ms), (30_000_000, 287_000));

        crate::feedback::set_like(&c, id, true).unwrap();
        let j = journal(&c).unwrap();
        assert!(
            j.iter()
                .any(|r| matches!(r, JournalRow::Mark { kind, track_id, .. } if kind == "like" && *track_id == id)),
            "{j:?}"
        );
    }
}
