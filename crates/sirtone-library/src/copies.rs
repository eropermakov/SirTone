//! Economy copies (D-146, M8.1; migration 0009): which tracks need a copy, where it lives, and
//! «Удалить оригиналы, оставить копии». Encoding is the caller's (the phone's composition root
//! has the audio crate); this module only keeps the database and the files in step.
//!
//! Only the phone calls [`keep_copy_only`]: on the PC originals are never removed this way.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::LibraryError;
use crate::repo::TrackId;

/// A track waiting for its copy, with what the copy's own tags need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyJob {
    pub id: TrackId,
    pub rel_path: String,
    pub title: String,
    pub artists: String,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    /// The album cover's original picture in the cover store.
    pub cover: Option<std::path::PathBuf>,
}

/// What the phone keeps («Хранилище»).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Storage {
    /// Original files (tracks that still have one).
    pub original_bytes: u64,
    /// Copies next to originals plus copy-only tracks.
    pub copy_bytes: u64,
    /// Tracks with both.
    pub both: u64,
    /// Tracks with the copy only.
    pub copy_only: u64,
    /// Tracks with the original only.
    pub original_only: u64,
}

/// The copy's place next to the original: same folder and name plus the track id, `.opus` —
/// two tracks with one stem (`X.flac`, `X.mp3`) never share a copy.
pub fn copy_rel_for(rel_path: &str, id: TrackId) -> String {
    let (dir, name) = rel_path
        .rsplit_once('/')
        .map_or(("", rel_path), |(d, n)| (d, n));
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let file = format!("{stem}.{id}.opus");
    if dir.is_empty() {
        file
    } else {
        format!("{dir}/{file}")
    }
}

const JOB_COLS: &str = "t.id, t.rel_path, t.title,
    coalesce((SELECT group_concat(name, ', ') FROM
      (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
       WHERE ta.track_id = t.id ORDER BY ta.position)), ''),
    al.title, (SELECT name FROM artist WHERE id = al.album_artist_id),
    t.track_no, t.disc_no, coalesce(t.year, al.year), t.genre_main,
    (SELECT hash FROM cover WHERE id = coalesce(al.cover_id, t.cover_id))";

fn job(root: &Path, r: &rusqlite::Row<'_>) -> rusqlite::Result<CopyJob> {
    let cover: Option<String> = r.get(10)?;
    Ok(CopyJob {
        id: r.get(0)?,
        rel_path: r.get(1)?,
        title: r.get(2)?,
        artists: r.get(3)?,
        album: r.get(4)?,
        album_artist: r.get(5)?,
        track_no: r.get(6)?,
        disc_no: r.get(7)?,
        year: r.get(8)?,
        genre: r.get(9)?,
        cover: cover.and_then(|h| crate::covers::original(root, &h)),
    })
}

/// One track's copy job (the hub makes copies for tracks it sends).
pub fn job_for(
    conn: &Connection,
    root: &Path,
    id: TrackId,
) -> Result<Option<CopyJob>, LibraryError> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {JOB_COLS} FROM track t LEFT JOIN album al ON al.id = t.album_id
                 WHERE t.id = ?1"
            ),
            [id],
            |r| job(root, r),
        )
        .optional()?)
}

/// Tracks that would gain from a copy at `kbps`: no copy yet, not a copy themselves, not Opus,
/// and not already about as small. `after` — only tracks with a larger id (a fresh import).
pub fn candidates(
    conn: &Connection,
    root: &Path,
    kbps: u32,
    after: TrackId,
    limit: u32,
) -> Result<Vec<CopyJob>, LibraryError> {
    let mut st = conn.prepare_cached(&format!(
        "SELECT {JOB_COLS}
         FROM track t LEFT JOIN album al ON al.id = t.album_id
         WHERE t.state = 'ok' AND t.id > ?1 AND t.copy_rel_path IS NULL AND t.is_copy = 0
           AND t.codec <> 'opus' AND (t.bitrate_kbps IS NULL OR t.bitrate_kbps > ?2)
         ORDER BY t.id LIMIT ?3"
    ))?;
    let rows = st
        .query_map(params![after, kbps + 32, limit], |r| job(root, r))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// The newest track id (an import's new tracks come after it).
pub fn last_id(conn: &Connection) -> Result<TrackId, LibraryError> {
    Ok(conn.query_row("SELECT coalesce(max(id), 0) FROM track", [], |r| r.get(0))?)
}

/// Records a copy made next to the original.
pub fn set_copy(
    conn: &Connection,
    id: TrackId,
    copy_rel: &str,
    bytes: u64,
    level: &str,
) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE track SET copy_rel_path = ?2, copy_bytes = ?3, copy_level = ?4 WHERE id = ?1",
        params![id, copy_rel, bytes as i64, level],
    )?;
    Ok(())
}

/// Marks a track whose file is itself a copy (it came that way from another device, Q-34).
pub fn mark_copy_only(conn: &Connection, id: TrackId, level: &str) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE track SET is_copy = 1, copy_level = ?2, copy_rel_path = NULL, copy_bytes = NULL
         WHERE id = ?1",
        params![id, level],
    )?;
    Ok(())
}

/// «Оставить только копию» for one track: the copy becomes the track's file, then the original
/// is removed. Returns the bytes freed (`None` — the track has no copy).
pub fn keep_copy_only(
    conn: &Connection,
    root: &Path,
    id: TrackId,
) -> Result<Option<u64>, LibraryError> {
    let row: Option<(String, Option<String>, i64, Option<String>)> = conn
        .query_row(
            "SELECT rel_path, copy_rel_path, size_bytes, copy_level FROM track
             WHERE id = ?1 AND is_copy = 0",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((original, Some(copy), size, level)) = row else {
        return Ok(None);
    };
    // The row describes the copy now (quality ranking must not see the original's numbers).
    let kbps = if level.as_deref() == Some("max") {
        64
    } else {
        128
    };
    let at = |rel: &str| rel.split('/').fold(root.to_path_buf(), |p, s| p.join(s));
    let bytes = std::fs::read(at(&copy))?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    conn.execute(
        "UPDATE track SET rel_path = ?2, size_bytes = ?3, content_hash = ?4, codec = 'opus',
                container = 'ogg', lossless = 0, is_copy = 1, bitrate_kbps = ?5,
                sample_rate = 48000, bit_depth = NULL,
                copy_rel_path = NULL, copy_bytes = NULL
         WHERE id = ?1",
        params![id, copy, bytes.len() as i64, hash, kbps],
    )?;
    let _ = std::fs::remove_file(at(&original));
    Ok(Some(size.max(0) as u64))
}

/// Tracks that have a copy next to the original (for «Удалить оригиналы, оставить копии»).
pub fn with_copy(conn: &Connection) -> Result<Vec<TrackId>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT id FROM track WHERE state = 'ok' AND is_copy = 0 AND copy_rel_path IS NOT NULL",
    )?;
    let ids = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(ids)
}

pub fn storage(conn: &Connection) -> Result<Storage, LibraryError> {
    Ok(conn.query_row(
        "SELECT
           coalesce(sum(CASE WHEN is_copy = 0 THEN size_bytes END), 0),
           coalesce(sum(CASE WHEN is_copy = 1 THEN size_bytes ELSE coalesce(copy_bytes, 0) END), 0),
           coalesce(sum(is_copy = 0 AND copy_rel_path IS NOT NULL), 0),
           coalesce(sum(is_copy = 1), 0),
           coalesce(sum(is_copy = 0 AND copy_rel_path IS NULL), 0)
         FROM track WHERE state = 'ok'",
        [],
        |r| {
            Ok(Storage {
                original_bytes: r.get::<_, i64>(0)?.max(0) as u64,
                copy_bytes: r.get::<_, i64>(1)?.max(0) as u64,
                both: r.get::<_, i64>(2)?.max(0) as u64,
                copy_only: r.get::<_, i64>(3)?.max(0) as u64,
                original_only: r.get::<_, i64>(4)?.max(0) as u64,
            })
        },
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{
        Identity, NewTrack, album_get_or_create, artist_get_or_create, track_insert,
    };

    #[test]
    fn copy_sits_next_to_the_original() {
        assert_eq!(
            copy_rel_for("Кино/Группа крови/01 Ёлка.flac", 7),
            "Кино/Группа крови/01 Ёлка.7.opus"
        );
        assert_ne!(
            copy_rel_for("a/X.flac", 1),
            copy_rel_for("a/X.mp3", 2),
            "one stem, two copies"
        );
        assert_eq!(copy_rel_for("t.opus", 3), "t.3.opus");
    }

    #[test]
    fn keeping_the_copy_frees_the_original() {
        let root = std::env::temp_dir().join(format!("sirtone-copies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Кино")).unwrap();
        std::fs::write(root.join("Кино/01 Ёлка.flac"), vec![0u8; 5000]).unwrap();
        let mut c =
            crate::db::open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap();
        let kino = artist_get_or_create(&c, "Кино").unwrap();
        let al = album_get_or_create(&c, "Ёлка", kino, None).unwrap();
        let id = track_insert(
            &mut c,
            &NewTrack {
                rel_path: "Кино/01 Ёлка.flac".into(),
                size_bytes: 5000,
                mtime_ms: 0,
                content_hash: "h".into(),
                container: "flac".into(),
                codec: "flac".into(),
                lossless: true,
                sample_rate: Some(44100),
                channels: Some(2),
                duration_ms: 1000,
                title: "Ёлка".into(),
                album_id: Some(al),
                disc_no: None,
                track_no: Some(1),
                year: None,
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![kino],
            },
        )
        .unwrap();
        let jobs = candidates(&c, &root, 128, 0, 10).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(
            (jobs[0].artists.as_str(), jobs[0].album.as_deref()),
            ("Кино", Some("Ёлка"))
        );
        let copy = copy_rel_for(&jobs[0].rel_path, id);
        std::fs::write(root.join(&copy), vec![1u8; 700]).unwrap();
        set_copy(&c, id, &copy, 700, "economy").unwrap();
        assert!(
            candidates(&c, &root, 128, 0, 10).unwrap().is_empty(),
            "no second copy"
        );
        let s = storage(&c).unwrap();
        assert_eq!((s.original_bytes, s.copy_bytes, s.both), (5000, 700, 1));
        assert_eq!(with_copy(&c).unwrap(), [id]);
        assert_eq!(keep_copy_only(&c, &root, id).unwrap(), Some(5000));
        assert!(!root.join("Кино/01 Ёлка.flac").exists());
        let s = storage(&c).unwrap();
        assert_eq!((s.original_bytes, s.copy_bytes, s.copy_only), (0, 700, 1));
        let rel: String = c
            .query_row("SELECT rel_path FROM track WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(rel, copy);
        assert_eq!(
            keep_copy_only(&c, &root, id).unwrap(),
            None,
            "already copy-only"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
