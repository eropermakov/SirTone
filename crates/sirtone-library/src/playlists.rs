//! Manual playlists (T2.11, FR-PLS-001/004; `01b` F-13 «Ручные»): references to tracks — removing
//! an item never touches the track or its file. Items are addressed by position (a track may be
//! in a playlist twice, Spotify «Add anyway»); positions stay dense `0..n` after every edit.
//! Tracks whose file is missing keep their row (they come back with the file) but are not listed.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::collection::{TRACK_COLS, track_row};
use crate::db::now_ms;
use crate::error::LibraryError;
use crate::repo::{TrackId, TrackRow};

pub type PlaylistId = i64;

/// A sidebar / list row.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistRow {
    pub id: PlaylistId,
    pub name: String,
    pub tracks: u32,
    pub duration_ms: u64,
    /// Four different album covers of the first tracks → a 2×2 mosaic; otherwise at most one
    /// (Spotify, `01b` F-13).
    pub covers: Vec<String>,
}

/// An item of a playlist page: its position (the address for remove / move) and the track.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaylistTrack {
    pub position: i64,
    pub track: TrackRow,
}

fn clean_name(name: &str) -> Result<String, LibraryError> {
    let n = name.trim();
    if n.is_empty() {
        return Err(LibraryError::Invalid("playlist name is empty".into()));
    }
    Ok(n.chars().take(200).collect())
}

fn exists(conn: &Connection, id: PlaylistId) -> Result<(), LibraryError> {
    conn.query_row(
        "SELECT 1 FROM playlist WHERE id = ?1 AND kind = 'manual'",
        [id],
        |_| Ok(()),
    )
    .optional()?
    .ok_or_else(|| LibraryError::Invalid(format!("no playlist {id}")))
}

fn touch(tx: &Transaction<'_>, id: PlaylistId) -> Result<(), LibraryError> {
    tx.execute(
        "UPDATE playlist SET updated_at = ?2 WHERE id = ?1",
        params![id, now_ms()],
    )?;
    Ok(())
}

/// All items in order: (track, added_at).
fn items(tx: &Transaction<'_>, id: PlaylistId) -> Result<Vec<(TrackId, i64)>, LibraryError> {
    let mut st = tx.prepare(
        "SELECT track_id, added_at FROM playlist_item WHERE playlist_id = ?1 ORDER BY position",
    )?;
    let rows = st
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Rewrites the items densely (delete + insert: `UPDATE position` would collide on the key).
fn rewrite(
    tx: &Transaction<'_>,
    id: PlaylistId,
    items: &[(TrackId, i64)],
) -> Result<(), LibraryError> {
    tx.execute("DELETE FROM playlist_item WHERE playlist_id = ?1", [id])?;
    let mut ins = tx.prepare(
        "INSERT INTO playlist_item(playlist_id, position, track_id, added_at) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (n, (track, added)) in items.iter().enumerate() {
        ins.execute(params![id, n as i64, track, added])?;
    }
    Ok(())
}

pub fn create(conn: &Connection, name: &str) -> Result<PlaylistId, LibraryError> {
    let name = clean_name(name)?;
    let now = now_ms();
    conn.execute(
        "INSERT INTO playlist(uid, name, kind, sort_order, created_at, updated_at)
         VALUES (?1, ?2, 'manual', (SELECT coalesce(max(sort_order), 0) + 1 FROM playlist), ?3, ?3)",
        params![uuid::Uuid::now_v7().to_string(), name, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename(conn: &Connection, id: PlaylistId, name: &str) -> Result<(), LibraryError> {
    let name = clean_name(name)?;
    exists(conn, id)?;
    conn.execute(
        "UPDATE playlist SET name = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, name, now_ms()],
    )?;
    Ok(())
}

/// The playlist goes; its tracks stay (FR-PLS-004).
pub fn delete(conn: &Connection, id: PlaylistId) -> Result<(), LibraryError> {
    exists(conn, id)?;
    conn.execute("DELETE FROM playlist WHERE id = ?1", [id])?;
    Ok(())
}

/// Manual playlists in creation order (`sort_order`), with what the sidebar shows.
pub fn list(conn: &Connection) -> Result<Vec<PlaylistRow>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT p.id, p.name,
                (SELECT count(*) FROM playlist_item i JOIN track t ON t.id = i.track_id
                 WHERE i.playlist_id = p.id AND t.state = 'ok'),
                (SELECT coalesce(sum(t.duration_ms), 0) FROM playlist_item i JOIN track t ON t.id = i.track_id
                 WHERE i.playlist_id = p.id AND t.state = 'ok')
         FROM playlist p WHERE p.kind = 'manual' ORDER BY p.sort_order, p.id",
    )?;
    let mut rows: Vec<PlaylistRow> = st
        .query_map([], |r| {
            Ok(PlaylistRow {
                id: r.get(0)?,
                name: r.get(1)?,
                tracks: r.get(2)?,
                duration_ms: r.get::<_, i64>(3)? as u64,
                covers: Vec::new(),
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut covers = conn.prepare_cached(
        "SELECT cv.hash FROM playlist_item i JOIN track t ON t.id = i.track_id
         JOIN album al ON al.id = t.album_id JOIN cover cv ON cv.id = al.cover_id
         WHERE i.playlist_id = ?1 AND t.state = 'ok' ORDER BY i.position LIMIT 64",
    )?;
    for row in &mut rows {
        let mut seen: Vec<String> = Vec::with_capacity(4);
        let mut q = covers.query([row.id])?;
        while let Some(r) = q.next()? {
            let h: String = r.get(0)?;
            if !seen.contains(&h) {
                seen.push(h);
                if seen.len() == 4 {
                    break;
                }
            }
        }
        seen.truncate(if seen.len() == 4 { 4 } else { 1 });
        row.covers = seen;
    }
    Ok(rows)
}

/// The playlist page: playable items in order with their positions.
pub fn tracks(conn: &Connection, id: PlaylistId) -> Result<Vec<PlaylistTrack>, LibraryError> {
    let mut st = conn.prepare_cached(&format!(
        "SELECT {TRACK_COLS}, i.position FROM playlist_item i JOIN track t ON t.id = i.track_id
         LEFT JOIN album al ON al.id = t.album_id
         WHERE i.playlist_id = ?1 AND t.state = 'ok' ORDER BY i.position"
    ))?;
    let rows = st
        .query_map([id], |r| {
            Ok(PlaylistTrack {
                track: track_row(r)?,
                position: r.get(8)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Which of `tracks` are already in the playlist (the «Уже в плейлисте» question).
pub fn already_in(
    conn: &Connection,
    id: PlaylistId,
    tracks: &[TrackId],
) -> Result<Vec<TrackId>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT 1 FROM playlist_item WHERE playlist_id = ?1 AND track_id = ?2 LIMIT 1",
    )?;
    let mut out = Vec::new();
    for &t in tracks {
        if st.exists(params![id, t])? && !out.contains(&t) {
            out.push(t);
        }
    }
    Ok(out)
}

/// Appends `tracks` in order. Returns how many were added.
pub fn add(conn: &mut Connection, id: PlaylistId, tracks: &[TrackId]) -> Result<u32, LibraryError> {
    let tx = conn.transaction()?;
    exists(&tx, id)?;
    let next: i64 = tx.query_row(
        "SELECT coalesce(max(position), -1) + 1 FROM playlist_item WHERE playlist_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let now = now_ms();
    let mut n = 0u32;
    {
        let mut ins = tx.prepare(
            "INSERT INTO playlist_item(playlist_id, position, track_id, added_at)
             SELECT ?1, ?2, id, ?4 FROM track WHERE id = ?3",
        )?;
        for &t in tracks {
            if ins.execute(params![id, next + i64::from(n), t, now])? == 1 {
                n += 1;
            }
        }
    }
    touch(&tx, id)?;
    tx.commit()?;
    Ok(n)
}

/// Removes the items at `positions` (the tracks stay — FR-PLS-004).
pub fn remove(
    conn: &mut Connection,
    id: PlaylistId,
    positions: &[i64],
) -> Result<(), LibraryError> {
    let tx = conn.transaction()?;
    exists(&tx, id)?;
    let kept: Vec<_> = items(&tx, id)?
        .into_iter()
        .enumerate()
        .filter(|(n, _)| !positions.contains(&(*n as i64)))
        .map(|(_, it)| it)
        .collect();
    rewrite(&tx, id, &kept)?;
    touch(&tx, id)?;
    tx.commit()?;
    Ok(())
}

/// Moves the item at `from` so that it ends up at `to` (both positions).
pub fn move_item(
    conn: &mut Connection,
    id: PlaylistId,
    from: i64,
    to: i64,
) -> Result<(), LibraryError> {
    let tx = conn.transaction()?;
    exists(&tx, id)?;
    let mut all = items(&tx, id)?;
    let n = all.len() as i64;
    if !(0..n).contains(&from) || !(0..n).contains(&to) {
        return Err(LibraryError::Invalid(format!("move {from} → {to} of {n}")));
    }
    if from != to {
        let it = all.remove(from as usize);
        all.insert(to as usize, it);
        rewrite(&tx, id, &all)?;
        touch(&tx, id)?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open;
    use crate::repo::{Identity, NewTrack, track_insert, track_remove};

    fn lib(tag: &str) -> (std::path::PathBuf, Connection) {
        let root = std::env::temp_dir().join(format!("sirtone-pl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let c = open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap();
        (root, c)
    }

    fn track(c: &mut Connection, n: u32) -> TrackId {
        track_insert(
            c,
            &NewTrack {
                rel_path: format!("A/{n:02}.mp3"),
                size_bytes: 5,
                mtime_ms: 0,
                content_hash: format!("h{n}"),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: 1000,
                title: format!("T{n}"),
                album_id: None,
                disc_no: None,
                track_no: None,
                year: None,
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![],
            },
        )
        .unwrap()
    }

    fn titles(c: &Connection, id: PlaylistId) -> Vec<String> {
        tracks(c, id)
            .unwrap()
            .into_iter()
            .map(|t| t.track.title)
            .collect()
    }

    #[test]
    fn create_add_move_remove_keep_positions_dense() {
        let (root, mut c) = lib("edit");
        let ids: Vec<_> = (1..=4).map(|n| track(&mut c, n)).collect();
        let p = create(&c, "  Мой плейлист 1 ").unwrap();
        assert!(create(&c, "   ").is_err(), "empty name");
        assert_eq!(add(&mut c, p, &ids).unwrap(), 4);
        // A duplicate is allowed (after the question in the UI).
        assert_eq!(already_in(&c, p, &[ids[0], 999]).unwrap(), vec![ids[0]]);
        add(&mut c, p, &[ids[0]]).unwrap();
        assert_eq!(titles(&c, p), ["T1", "T2", "T3", "T4", "T1"]);
        move_item(&mut c, p, 0, 3).unwrap(); // down
        assert_eq!(titles(&c, p), ["T2", "T3", "T4", "T1", "T1"]);
        move_item(&mut c, p, 4, 0).unwrap(); // up
        assert_eq!(titles(&c, p), ["T1", "T2", "T3", "T4", "T1"]);
        assert!(move_item(&mut c, p, 0, 9).is_err());
        remove(&mut c, p, &[1, 4]).unwrap();
        let pos: Vec<i64> = tracks(&c, p).unwrap().iter().map(|t| t.position).collect();
        assert_eq!(
            (titles(&c, p), pos),
            (vec!["T1".into(), "T3".into(), "T4".into()], vec![0, 1, 2])
        );
        rename(&c, p, "Дорога").unwrap();
        let row = &list(&c).unwrap()[0];
        assert_eq!(
            (row.name.as_str(), row.tracks, row.duration_ms),
            ("Дорога", 3, 3000)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn playlists_are_references_to_tracks() {
        let (root, mut c) = lib("refs");
        let a = track(&mut c, 1);
        let b = track(&mut c, 2);
        let p1 = create(&c, "Один").unwrap();
        let p2 = create(&c, "Два").unwrap();
        add(&mut c, p1, &[a, b]).unwrap();
        add(&mut c, p2, &[a]).unwrap();
        // FR-PLS-004: removing from one playlist keeps the track and the other playlist.
        remove(&mut c, p1, &[0]).unwrap();
        let alive: i64 = c
            .query_row("SELECT count(*) FROM track WHERE id = ?1", [a], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!((alive, titles(&c, p2)), (1, vec!["T1".to_string()]));
        // Deleting the playlist keeps the tracks.
        delete(&c, p1).unwrap();
        let n: i64 = c
            .query_row("SELECT count(*) FROM track", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
        // A track leaving the library leaves the playlists (ON DELETE CASCADE).
        track_remove(&mut c, a).unwrap();
        assert!(titles(&c, p2).is_empty());
        // Missing files are hidden, not dropped.
        add(&mut c, p2, &[b]).unwrap();
        c.execute("UPDATE track SET state = 'missing' WHERE id = ?1", [b])
            .unwrap();
        assert!(titles(&c, p2).is_empty());
        c.execute("UPDATE track SET state = 'ok' WHERE id = ?1", [b])
            .unwrap();
        assert_eq!(titles(&c, p2), ["T2"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
