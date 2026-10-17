//! Manual assignment (T2.9 «Неизвестные», later T2.10 track card; docs/05 §4.7 «ввести вручную»,
//! Picard «перетащить/назначить»): the user says who and what a track is; the file moves to its
//! canonical place through `fs_journal` and the row changes in one transaction — the audio file
//! itself is never modified (principle 5: no tag writing). Runs on the import worker, so it never
//! races the watcher's reconcile (D-70).

use std::fs;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use sirtone_core::ports::FileOps;
use sirtone_core::text::norm;

use crate::db::now_ms;
use crate::error::LibraryError;
use crate::layout::{self, TrackPlace};
use crate::repo::{self, ArtistId, TrackId};

/// What the user filled in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Assignment {
    pub title: String,
    /// One or several artists separated by commas; the first is the primary one.
    pub artists: String,
    pub album: Option<String>,
    pub track_no: Option<u32>,
    pub year: Option<i32>,
}

fn from_rel(root: &Path, rel: &str) -> std::path::PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c))
}

fn clean(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Applies the assignment: canonical path, durable move, then the row. Returns the new path.
pub fn assign(
    conn: &mut Connection,
    root: &Path,
    track: TrackId,
    a: &Assignment,
    fsops: &dyn FileOps,
) -> Result<String, LibraryError> {
    let title = clean(&a.title).ok_or_else(|| LibraryError::Invalid("title is empty".into()))?;
    let names: Vec<String> = a.artists.split(',').filter_map(clean).collect();
    let primary = names
        .first()
        .cloned()
        .ok_or_else(|| LibraryError::Invalid("artist is empty".into()))?;
    let album = a.album.as_deref().and_then(clean);
    let old_rel: String = conn
        .query_row("SELECT rel_path FROM track WHERE id = ?1", [track], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| LibraryError::Invalid(format!("no track {track}")))?;
    let ext = Path::new(&old_rel)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let placement = layout::canonical(&TrackPlace {
        artist: &primary,
        album_artist: None,
        album: album.as_deref(),
        title: &title,
        track_no: a.track_no,
        disc_no: None,
        multi_disc: false,
        ext: &ext,
    });
    let rel = if placement.rel_path.eq_ignore_ascii_case(&old_rel) {
        old_rel.clone()
    } else {
        layout::deduplicate(&placement.rel_path, |c| {
            from_rel(root, c).exists()
                || conn
                    .query_row(
                        "SELECT 1 FROM track WHERE rel_path = ?1 COLLATE NOCASE AND id <> ?2",
                        params![c, track],
                        |_| Ok(()),
                    )
                    .optional()
                    .ok()
                    .flatten()
                    .is_some()
        })
    };

    let journal = if rel == old_rel {
        None
    } else {
        let (src, dst) = (from_rel(root, &old_rel), from_rel(root, &rel));
        if let Some(dir) = dst.parent() {
            fs::create_dir_all(dir)?;
        }
        conn.execute(
            "INSERT INTO fs_journal(op, src, dst, state, ts) VALUES ('move', ?1, ?2, 'planned', ?3)",
            params![old_rel, rel, now_ms()],
        )?;
        let id = conn.last_insert_rowid();
        if let Err(e) = fsops.move_durable(&src, &dst) {
            conn.execute(
                "UPDATE fs_journal SET state = 'rolled_back' WHERE id = ?1",
                [id],
            )?;
            return Err(e.into());
        }
        Some(id)
    };

    let tx = conn.transaction()?;
    let artist_ids = names
        .iter()
        .map(|n| repo::artist_get_or_create(&tx, n))
        .collect::<Result<Vec<_>, _>>()?;
    let album_id = match (&album, artist_ids.first()) {
        (Some(al), Some(&aa)) => Some(repo::album_get_or_create(&tx, al, aa, a.year)?),
        _ => None,
    };
    tx.execute(
        "UPDATE track SET title = ?2, title_norm = ?3, album_id = ?4, track_no = ?5, year = ?6,
                identity_state = 'user', identity_conf = 1, unknown_reason = NULL,
                candidates_json = NULL, rel_path = ?7
         WHERE id = ?1",
        params![
            track,
            title,
            norm(&title).unwrap_or_default(),
            album_id,
            a.track_no,
            a.year,
            rel
        ],
    )?;
    repo::set_track_artists(&tx, track, &artist_ids)?;
    repo::search_reindex(&tx, track)?;
    if let Some(id) = journal {
        tx.execute("UPDATE fs_journal SET state = 'done' WHERE id = ?1", [id])?;
    }
    tx.commit()?;
    Ok(rel)
}

/// FR-DATA-001: a track leaves the library. `with_file` — its file goes to the Recycle Bin
/// (never a hard delete, through the journal); otherwise only the row goes and the file is
/// remembered in `fs_ignore`, so the watcher does not take it back in.
pub fn remove(
    conn: &mut Connection,
    root: &Path,
    track: TrackId,
    with_file: bool,
    fsops: &dyn FileOps,
) -> Result<(), LibraryError> {
    let (rel, size, mtime): (String, i64, i64) = conn
        .query_row(
            "SELECT rel_path, size_bytes, mtime_ms FROM track WHERE id = ?1",
            [track],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| LibraryError::Invalid(format!("no track {track}")))?;
    let path = from_rel(root, &rel);
    if with_file && path.exists() {
        conn.execute(
            "INSERT INTO fs_journal(op, src, dst, state, ts) VALUES ('trash', ?1, NULL, 'planned', ?2)",
            params![rel, now_ms()],
        )?;
        let id = conn.last_insert_rowid();
        if let Err(e) = fsops.to_recycle_bin(&path) {
            conn.execute(
                "UPDATE fs_journal SET state = 'rolled_back' WHERE id = ?1",
                [id],
            )?;
            return Err(e.into());
        }
        repo::track_remove(conn, track)?;
        conn.execute("UPDATE fs_journal SET state = 'done' WHERE id = ?1", [id])?;
    } else {
        if path.exists() {
            conn.execute(
                "INSERT INTO fs_ignore(rel_path, size_bytes, mtime_ms, reason, ts) VALUES (?1, ?2, ?3, 'removed_by_user', ?4)
                 ON CONFLICT(rel_path) DO UPDATE SET size_bytes = excluded.size_bytes,
                   mtime_ms = excluded.mtime_ms, reason = excluded.reason, ts = excluded.ts",
                params![rel, size, mtime, now_ms()],
            )?;
        }
        repo::track_remove(conn, track)?;
    }
    Ok(())
}

/// FR-DATA-002: the tracks on an artist's page (as the card counts them) and their total size,
/// for the confirmation «N треков, X МБ».
pub fn artist_tracks(
    conn: &Connection,
    artist: ArtistId,
) -> Result<(Vec<TrackId>, u64), LibraryError> {
    let mut st = conn.prepare(
        "SELECT t.id, t.size_bytes FROM track_artist ta JOIN track t ON t.id = ta.track_id
         WHERE ta.artist_id = ?1 AND t.state = 'ok' ORDER BY t.id",
    )?;
    let mut ids = Vec::new();
    let mut bytes = 0u64;
    let mut q = st.query([artist])?;
    while let Some(r) = q.next()? {
        ids.push(r.get(0)?);
        bytes += u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0);
    }
    Ok((ids, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open;
    use crate::repo::{Identity, NewTrack, track_insert};

    struct Ops;
    impl FileOps for Ops {
        fn move_durable(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            if to.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "exists",
                ));
            }
            fs::rename(from, to)
        }
        fn to_recycle_bin(&self, _: &Path) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn unknown_track_moves_to_its_canonical_place() {
        let root = std::env::temp_dir().join(format!("sirtone-assign-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("_Unknown")).unwrap();
        fs::write(root.join("_Unknown/track07.mp3"), b"audio").unwrap();
        let mut c = open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap();
        let id = track_insert(
            &mut c,
            &NewTrack {
                rel_path: "_Unknown/track07.mp3".into(),
                size_bytes: 5,
                mtime_ms: 0,
                content_hash: "h".into(),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: 1000,
                title: "track07".into(),
                album_id: None,
                disc_no: None,
                track_no: None,
                year: None,
                identity: Identity::Unknown,
                identity_conf: 0.1,
                unknown_reason: Some("no_metadata".into()),
                artists: vec![],
            },
        )
        .unwrap();
        let rel = assign(
            &mut c,
            &root,
            id,
            &Assignment {
                title: "Группа крови".into(),
                artists: "Кино".into(),
                album: Some("Группа крови".into()),
                track_no: Some(1),
                year: Some(1988),
            },
            &Ops,
        )
        .unwrap();
        assert_eq!(rel, "Кино/Группа крови/01 - Группа крови.mp3");
        assert!(root.join(&rel).exists() && !root.join("_Unknown/track07.mp3").exists());
        let (state, reason, title): (String, Option<String>, String) = c
            .query_row(
                "SELECT identity_state, unknown_reason, title FROM track WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (state.as_str(), reason, title.as_str()),
            ("user", None, "Группа крови")
        );
        let journal: String = c
            .query_row(
                "SELECT state FROM fs_journal ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(journal, "done");
        assert!(
            assign(&mut c, &root, id, &Assignment::default(), &Ops).is_err(),
            "empty title"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn artist_removal_only_from_the_app_keeps_files_and_ignores_them() {
        let root = std::env::temp_dir().join(format!("sirtone-remove-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Кино")).unwrap();
        let mut c = open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap();
        let artist = repo::artist_get_or_create(&c, "Кино").unwrap();
        let mut ids = Vec::new();
        for n in 1..=2 {
            let rel = format!("Кино/{n:02}.mp3");
            fs::write(root.join(&rel), b"audio").unwrap();
            let id = track_insert(
                &mut c,
                &NewTrack {
                    rel_path: rel,
                    size_bytes: 1000,
                    mtime_ms: 0,
                    content_hash: format!("h{n}"),
                    container: "mp3".into(),
                    codec: "mp3".into(),
                    lossless: false,
                    sample_rate: None,
                    channels: None,
                    duration_ms: 1000,
                    title: format!("{n}"),
                    album_id: None,
                    disc_no: None,
                    track_no: None,
                    year: None,
                    identity: Identity::Inferred,
                    identity_conf: 0.9,
                    unknown_reason: None,
                    artists: vec![artist],
                },
            )
            .unwrap();
            ids.push(id);
        }
        let (found, bytes) = artist_tracks(&c, artist).unwrap();
        assert_eq!((found.clone(), bytes), (ids.clone(), 2000));
        for id in found {
            remove(&mut c, &root, id, false, &Ops).unwrap();
        }
        assert!(artist_tracks(&c, artist).unwrap().0.is_empty());
        assert!(root.join("Кино/01.mp3").exists(), "the file stays on disk");
        let ignored: i64 = c
            .query_row(
                "SELECT count(*) FROM fs_ignore WHERE reason = 'removed_by_user'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ignored, 2, "the watcher must not import them again");
        let _ = fs::remove_dir_all(&root);
    }
}
