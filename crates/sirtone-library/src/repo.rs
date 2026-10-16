//! Repositories over `library.db` (T1.9). Only parameters and whitelisted columns reach SQL.

use rusqlite::{Connection, OptionalExtension, params};
use sirtone_core::text::norm;

use crate::db::now_ms;
use crate::error::LibraryError;

pub type ArtistId = i64;
pub type AlbumId = i64;
pub type TrackId = i64;

/// Identity state of a track (docs/03 §7, docs/05 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identity {
    Confirmed,
    Inferred,
    User,
    Unknown,
}

impl Identity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Inferred => "inferred",
            Self::User => "user",
            Self::Unknown => "unknown",
        }
    }
}

fn key(s: &str) -> Result<String, LibraryError> {
    norm(s).ok_or_else(|| LibraryError::Invalid(format!("empty name `{s}`")))
}

/// Finds the canonical artist by `norm(name)` (auto-merge «Би-2» = «Би 2», docs/05 §1) or creates it.
pub fn artist_get_or_create(conn: &Connection, name: &str) -> Result<ArtistId, LibraryError> {
    let k = key(name)?;
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM artist WHERE name_norm = ?1 AND merged_into IS NULL",
            [&k],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    if let Some(id) = conn
        .query_row(
            "SELECT artist_id FROM artist_alias WHERE alias_norm = ?1",
            [&k],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO artist(uid, name, name_norm, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![uuid::Uuid::now_v7().to_string(), name.trim(), k, now_ms()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn album_get_or_create(
    conn: &Connection,
    title: &str,
    album_artist: ArtistId,
    year: Option<i32>,
) -> Result<AlbumId, LibraryError> {
    let k = key(title)?;
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM album WHERE album_artist_id = ?1 AND title_norm = ?2",
            params![album_artist, k],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO album(uid, title, title_norm, album_artist_id, year, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            uuid::Uuid::now_v7().to_string(),
            title.trim(),
            k,
            album_artist,
            year,
            now_ms()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// What the import pipeline knows about a placed file.
#[derive(Debug, Clone)]
pub struct NewTrack {
    pub rel_path: String,
    pub size_bytes: u64,
    pub mtime_ms: i64,
    pub content_hash: String,
    pub container: String,
    pub codec: String,
    pub lossless: bool,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub duration_ms: u64,
    pub title: String,
    pub album_id: Option<AlbumId>,
    pub disc_no: Option<u32>,
    pub track_no: Option<u32>,
    pub year: Option<i32>,
    pub identity: Identity,
    pub identity_conf: f64,
    pub unknown_reason: Option<String>,
    /// Primary first, then featured (docs/05 §5).
    pub artists: Vec<ArtistId>,
}

/// Inserts the track, its artists and both search rows in one transaction.
pub fn track_insert(conn: &mut Connection, t: &NewTrack) -> Result<TrackId, LibraryError> {
    let tx = conn.transaction()?;
    let id = track_insert_in(&tx, t)?;
    tx.commit()?;
    Ok(id)
}

/// [`track_insert`] inside a caller's transaction (import places a file and indexes it atomically).
pub fn track_insert_in(tx: &Connection, t: &NewTrack) -> Result<TrackId, LibraryError> {
    let title_norm = norm(&t.title).unwrap_or_default();
    tx.execute(
        "INSERT INTO track(uid, rel_path, size_bytes, mtime_ms, content_hash, container, codec, lossless,
           sample_rate, channels, duration_ms, title, title_norm, album_id, disc_no, track_no, year,
           identity_state, identity_conf, unknown_reason, added_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
        params![
            uuid::Uuid::now_v7().to_string(),
            t.rel_path,
            t.size_bytes as i64,
            t.mtime_ms,
            t.content_hash,
            t.container,
            t.codec,
            t.lossless,
            t.sample_rate,
            t.channels,
            t.duration_ms as i64,
            t.title,
            title_norm,
            t.album_id,
            t.disc_no,
            t.track_no,
            t.year,
            t.identity.as_str(),
            t.identity_conf,
            t.unknown_reason,
            now_ms(),
        ],
    )?;
    let id = tx.last_insert_rowid();
    set_track_artists(tx, id, &t.artists)?;
    search_reindex(tx, id)?;
    Ok(id)
}

fn set_track_artists(
    conn: &Connection,
    id: TrackId,
    artists: &[ArtistId],
) -> Result<(), LibraryError> {
    conn.execute(
        "DELETE FROM track_artist WHERE track_id = ?1 AND role IN ('primary','featured')",
        [id],
    )?;
    for (pos, a) in artists.iter().enumerate() {
        let role = if pos == 0 { "primary" } else { "featured" };
        conn.execute(
            "INSERT OR IGNORE INTO track_artist(track_id, artist_id, role, position) VALUES (?1, ?2, ?3, ?4)",
            params![id, a, role, pos as i64],
        )?;
    }
    Ok(())
}

/// Rebuilds both search rows of one track from the current data (T1.10). Contentless tables with
/// `contentless_delete=1` (D-65) allow replacing a row by `rowid`.
pub fn search_reindex(conn: &Connection, id: TrackId) -> Result<(), LibraryError> {
    conn.execute("DELETE FROM search_fts WHERE rowid = ?1", [id])?;
    conn.execute("DELETE FROM search_tri WHERE rowid = ?1", [id])?;
    let row = conn
        .query_row(
            "SELECT t.title, coalesce(al.title, ''), coalesce(t.genre_main, ''),
                    coalesce((SELECT group_concat(name, ' ') FROM
                      (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                       WHERE ta.track_id = t.id ORDER BY ta.position)), ''),
                    coalesce((SELECT group_concat(alias_norm, ' ') FROM artist_alias aa
                       JOIN track_artist ta ON ta.artist_id = aa.artist_id WHERE ta.track_id = t.id), ''),
                    t.state
             FROM track t LEFT JOIN album al ON al.id = t.album_id WHERE t.id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?;
    // Deleted or not playable tracks are not searchable.
    let Some((title, album, genre, artists, aliases, state)) = row else {
        return Ok(());
    };
    if state != "ok" {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO search_fts(rowid, title, artists, album, genre, aliases) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, title, artists, album, genre, aliases],
    )?;
    let tri = norm(&format!("{title} {artists} {album}")).unwrap_or_default();
    conn.execute(
        "INSERT INTO search_tri(rowid, text) VALUES (?1, ?2)",
        params![id, tri],
    )?;
    Ok(())
}

/// User or recognition edit of a track's identity (FR-REC, «Неизвестные»).
#[derive(Debug, Clone)]
pub struct TrackEdit {
    pub title: String,
    pub album_id: Option<AlbumId>,
    pub artists: Vec<ArtistId>,
    pub genre_main: Option<String>,
    pub identity: Identity,
}

pub fn track_edit(conn: &mut Connection, id: TrackId, e: &TrackEdit) -> Result<(), LibraryError> {
    let tx = conn.transaction()?;
    let n = tx.execute(
        "UPDATE track SET title = ?2, title_norm = ?3, album_id = ?4, genre_main = ?5, identity_state = ?6
         WHERE id = ?1",
        params![id, e.title, norm(&e.title).unwrap_or_default(), e.album_id, e.genre_main, e.identity.as_str()],
    )?;
    if n == 0 {
        return Err(LibraryError::Invalid(format!("no track {id}")));
    }
    set_track_artists(&tx, id, &e.artists)?;
    search_reindex(&tx, id)?;
    tx.commit()?;
    Ok(())
}

/// «Только убрать из приложения» (FR-DATA-001): the row goes, the file stays.
pub fn track_remove(conn: &mut Connection, id: TrackId) -> Result<(), LibraryError> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM search_fts WHERE rowid = ?1", [id])?;
    tx.execute("DELETE FROM search_tri WHERE rowid = ?1", [id])?;
    tx.execute("DELETE FROM track WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(())
}

/// A row as the track list shows it (TrackRow, DESIGN.md §9).
#[derive(Debug, Clone, PartialEq)]
pub struct TrackRow {
    pub id: TrackId,
    pub title: String,
    pub artists: String,
    pub album: Option<String>,
    pub duration_ms: u64,
    pub rel_path: String,
}

/// Keyset cursor for «by title» pages (S-15: p95 0.9 ms on 100 000 tracks).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TitleCursor {
    pub title_norm: String,
    pub id: TrackId,
}

pub fn tracks_by_title(
    conn: &Connection,
    after: Option<&TitleCursor>,
    limit: u32,
) -> Result<(Vec<TrackRow>, Option<TitleCursor>), LibraryError> {
    let start = after.cloned().unwrap_or_default();
    let mut st = conn.prepare_cached(
        "SELECT t.id, t.title, t.title_norm, t.duration_ms, al.title, t.rel_path,
                (SELECT group_concat(name, ', ') FROM
                   (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                    WHERE ta.track_id = t.id ORDER BY ta.position))
         FROM track t LEFT JOIN album al ON al.id = t.album_id
         WHERE t.state = 'ok' AND (t.title_norm, t.id) > (?1, ?2)
         ORDER BY t.title_norm, t.id LIMIT ?3",
    )?;
    let mut rows = Vec::new();
    let mut next = None;
    let mut q = st.query(params![start.title_norm, start.id, limit])?;
    while let Some(r) = q.next()? {
        let id: TrackId = r.get(0)?;
        let title_norm: String = r.get(2)?;
        rows.push(TrackRow {
            id,
            title: r.get(1)?,
            duration_ms: r.get::<_, i64>(3)? as u64,
            album: r.get(4)?,
            rel_path: r.get(5)?,
            artists: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        });
        next = Some(TitleCursor { title_norm, id });
    }
    if rows.len() < limit as usize {
        next = None;
    }
    Ok((rows, next))
}

pub fn track_count(conn: &Connection) -> Result<u64, LibraryError> {
    Ok(
        conn.query_row("SELECT count(*) FROM track WHERE state = 'ok'", [], |r| {
            r.get::<_, i64>(0)
        })? as u64,
    )
}

pub fn track_by_rel_path(
    conn: &Connection,
    rel_path: &str,
) -> Result<Option<TrackId>, LibraryError> {
    Ok(conn
        .query_row(
            "SELECT id FROM track WHERE rel_path = ?1",
            [rel_path],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn track_by_hash(conn: &Connection, hash: &str) -> Result<Option<TrackId>, LibraryError> {
    Ok(conn
        .query_row(
            "SELECT id FROM track WHERE content_hash = ?1 LIMIT 1",
            [hash],
            |r| r.get(0),
        )
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn track(title: &str, path: &str, album: Option<AlbumId>, artists: Vec<ArtistId>) -> NewTrack {
        NewTrack {
            rel_path: path.into(),
            size_bytes: 1000,
            mtime_ms: 0,
            content_hash: format!("hash-{path}"),
            container: "flac".into(),
            codec: "flac".into(),
            lossless: true,
            sample_rate: Some(44100),
            channels: Some(2),
            duration_ms: 180_000,
            title: title.into(),
            album_id: album,
            disc_no: None,
            track_no: Some(1),
            year: None,
            identity: Identity::Inferred,
            identity_conf: 0.8,
            unknown_reason: None,
            artists,
        }
    }

    #[test]
    fn artists_auto_merge_by_norm() {
        let c = open_in_memory().unwrap();
        let a = artist_get_or_create(&c, "Би-2").unwrap();
        assert_eq!(artist_get_or_create(&c, "Би 2").unwrap(), a);
        assert_eq!(artist_get_or_create(&c, "  БИ-2 ").unwrap(), a);
        assert_ne!(artist_get_or_create(&c, "Bi-2").unwrap(), a); // translit = suggestion only
        assert!(artist_get_or_create(&c, " — ").is_err());
    }

    #[test]
    fn insert_page_and_search_rows() {
        let mut c = open_in_memory().unwrap();
        let bi2 = artist_get_or_create(&c, "Би-2").unwrap();
        let zem = artist_get_or_create(&c, "Земфира").unwrap();
        let alb = album_get_or_create(&c, "Иномарки", bi2, Some(2004)).unwrap();
        assert_eq!(album_get_or_create(&c, "иномарки", bi2, None).unwrap(), alb);
        for (i, t) in [
            "Варвара",
            "Полковнику никто не пишет",
            "Ёлка",
            "Мой рок-н-ролл",
        ]
        .iter()
        .enumerate()
        {
            let artists = if i == 0 { vec![bi2, zem] } else { vec![bi2] };
            track_insert(
                &mut c,
                &track(
                    t,
                    &format!("Би-2/Иномарки/{i:02} - {t}.flac"),
                    Some(alb),
                    artists,
                ),
            )
            .unwrap();
        }
        assert_eq!(track_count(&c).unwrap(), 4);
        let (page, next) = tracks_by_title(&c, None, 3).unwrap();
        let titles: Vec<_> = page.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Варвара", "Ёлка", "Мой рок-н-ролл"]); // ё sorts as е
        assert_eq!(page[0].artists, "Би-2, Земфира");
        assert_eq!(page[0].album.as_deref(), Some("Иномарки"));
        let (rest, end) = tracks_by_title(&c, next.as_ref(), 3).unwrap();
        assert_eq!(rest.len(), 1);
        assert!(end.is_none());
        let hits: i64 = c
            .query_row(
                "SELECT count(*) FROM search_fts WHERE search_fts MATCH 'земфира'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);
        assert!(
            track_by_rel_path(&c, "Би-2/Иномарки/00 - Варвара.flac")
                .unwrap()
                .is_some()
        );
        // unique rel_path
        assert!(
            track_insert(
                &mut c,
                &track("X", "Би-2/Иномарки/00 - Варвара.flac", None, vec![bi2])
            )
            .is_err()
        );
    }
}
