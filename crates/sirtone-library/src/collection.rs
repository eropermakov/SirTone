//! Collection read API (T1.25, FR-LIB-001/002/005): keyset pages of 100 and counters for the
//! Collection screens. Budget P-08: a page ≤ 10 ms on 100 000 tracks (S-15: 0.9 ms).

use rusqlite::{Connection, params};

use crate::error::LibraryError;
use crate::repo::{AlbumId, ArtistId, TrackRow};

#[derive(Debug, Clone, PartialEq)]
pub struct ArtistRow {
    pub id: ArtistId,
    pub name: String,
    pub tracks: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlbumRow {
    pub id: AlbumId,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub tracks: u32,
    pub cover_id: Option<i64>,
}

/// Keyset cursor: the sort key and id of the last row of the previous page.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cursor {
    pub key: String,
    pub id: i64,
}

/// «Вся коллекция: N треков · Ч ч М мин · X ГБ» (docs/06 §4) plus section counters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Counts {
    pub tracks: u64,
    pub artists: u64,
    pub albums: u64,
    pub unknown: u64,
    pub duration_ms: u64,
    pub bytes: u64,
}

fn finish<T>(rows: Vec<T>, last: Option<Cursor>, limit: u32) -> (Vec<T>, Option<Cursor>) {
    let next = if rows.len() == limit as usize {
        last
    } else {
        None
    };
    (rows, next)
}

/// Canonical artists that have at least one playable track, by name.
pub fn artists_page(
    conn: &Connection,
    after: Option<&Cursor>,
    limit: u32,
) -> Result<(Vec<ArtistRow>, Option<Cursor>), LibraryError> {
    let c = after.cloned().unwrap_or_default();
    let mut st = conn.prepare_cached(
        "SELECT a.id, a.name, a.name_norm,
                (SELECT count(*) FROM track_artist ta JOIN track t ON t.id = ta.track_id
                 WHERE ta.artist_id = a.id AND t.state = 'ok') AS n
         FROM artist a
         WHERE a.merged_into IS NULL AND (a.name_norm, a.id) > (?1, ?2) AND n > 0
         ORDER BY a.name_norm, a.id LIMIT ?3",
    )?;
    let mut rows = Vec::new();
    let mut last = None;
    let mut q = st.query(params![c.key, c.id, limit])?;
    while let Some(r) = q.next()? {
        let id = r.get(0)?;
        last = Some(Cursor { key: r.get(2)?, id });
        rows.push(ArtistRow {
            id,
            name: r.get(1)?,
            tracks: r.get(3)?,
        });
    }
    Ok(finish(rows, last, limit))
}

pub fn albums_page(
    conn: &Connection,
    after: Option<&Cursor>,
    limit: u32,
) -> Result<(Vec<AlbumRow>, Option<Cursor>), LibraryError> {
    let c = after.cloned().unwrap_or_default();
    let mut st = conn.prepare_cached(
        "SELECT al.id, al.title, al.title_norm, a.name, al.year, al.cover_id,
                (SELECT count(*) FROM track t WHERE t.album_id = al.id AND t.state = 'ok') AS n
         FROM album al JOIN artist a ON a.id = al.album_artist_id
         WHERE (al.title_norm, al.id) > (?1, ?2) AND n > 0
         ORDER BY al.title_norm, al.id LIMIT ?3",
    )?;
    let mut rows = Vec::new();
    let mut last = None;
    let mut q = st.query(params![c.key, c.id, limit])?;
    while let Some(r) = q.next()? {
        let id = r.get(0)?;
        last = Some(Cursor { key: r.get(2)?, id });
        rows.push(AlbumRow {
            id,
            title: r.get(1)?,
            artist: r.get(3)?,
            year: r.get(4)?,
            cover_id: r.get(5)?,
            tracks: r.get(6)?,
        });
    }
    Ok(finish(rows, last, limit))
}

const TRACK_COLS: &str = "t.id, t.title, t.duration_ms, al.title, t.rel_path,
    coalesce((SELECT group_concat(name, ', ') FROM
      (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
       WHERE ta.track_id = t.id ORDER BY ta.position)), '')";

fn track_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TrackRow> {
    Ok(TrackRow {
        id: r.get(0)?,
        title: r.get(1)?,
        duration_ms: r.get::<_, i64>(2)? as u64,
        album: r.get(3)?,
        rel_path: r.get(4)?,
        artists: r.get(5)?,
    })
}

/// Album tracks in disc/track order (ix_track_album).
pub fn album_tracks(conn: &Connection, album: AlbumId) -> Result<Vec<TrackRow>, LibraryError> {
    let mut st = conn.prepare_cached(&format!(
        "SELECT {TRACK_COLS} FROM track t LEFT JOIN album al ON al.id = t.album_id
         WHERE t.album_id = ?1 AND t.state = 'ok'
         ORDER BY coalesce(t.disc_no, 1), coalesce(t.track_no, 0), t.title_norm"
    ))?;
    let rows = st
        .query_map([album], track_row)?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// All tracks of an artist (primary or featured), by album then track number.
pub fn artist_tracks(conn: &Connection, artist: ArtistId) -> Result<Vec<TrackRow>, LibraryError> {
    let mut st = conn.prepare_cached(&format!(
        "SELECT {TRACK_COLS} FROM track_artist x JOIN track t ON t.id = x.track_id
         LEFT JOIN album al ON al.id = t.album_id
         WHERE x.artist_id = ?1 AND t.state = 'ok'
         ORDER BY al.year, al.title_norm, coalesce(t.disc_no, 1), coalesce(t.track_no, 0)"
    ))?;
    let rows = st
        .query_map([artist], track_row)?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// «Неизвестные» (FR-LIB-004): playable tracks without a confident identity, newest first.
pub fn unknown_tracks(conn: &Connection, limit: u32) -> Result<Vec<TrackRow>, LibraryError> {
    let mut st = conn.prepare_cached(&format!(
        "SELECT {TRACK_COLS} FROM track t LEFT JOIN album al ON al.id = t.album_id
         WHERE t.state = 'ok' AND t.identity_state = 'unknown'
         ORDER BY t.added_at DESC, t.id DESC LIMIT ?1"
    ))?;
    let rows = st
        .query_map([limit], track_row)?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

pub fn counts(conn: &Connection) -> Result<Counts, LibraryError> {
    Ok(conn.query_row(
        "SELECT
           (SELECT count(*) FROM track WHERE state = 'ok'),
           (SELECT count(DISTINCT ta.artist_id) FROM track_artist ta JOIN track t ON t.id = ta.track_id WHERE t.state = 'ok'),
           (SELECT count(DISTINCT album_id) FROM track WHERE state = 'ok' AND album_id IS NOT NULL),
           (SELECT count(*) FROM track WHERE state = 'ok' AND identity_state = 'unknown'),
           (SELECT coalesce(sum(duration_ms), 0) FROM track WHERE state = 'ok'),
           (SELECT coalesce(sum(size_bytes), 0) FROM track WHERE state = 'ok')",
        [],
        |r| {
            Ok(Counts {
                tracks: r.get::<_, i64>(0)? as u64,
                artists: r.get::<_, i64>(1)? as u64,
                albums: r.get::<_, i64>(2)? as u64,
                unknown: r.get::<_, i64>(3)? as u64,
                duration_ms: r.get::<_, i64>(4)? as u64,
                bytes: r.get::<_, i64>(5)? as u64,
            })
        },
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::repo::*;

    fn add(
        c: &mut Connection,
        title: &str,
        artists: Vec<ArtistId>,
        album: Option<AlbumId>,
        no: u32,
    ) -> TrackId {
        track_insert(
            c,
            &NewTrack {
                rel_path: format!("x/{title}-{no}.mp3"),
                size_bytes: 5_000_000,
                mtime_ms: 0,
                content_hash: format!("h-{title}-{no}"),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: Some(44100),
                channels: Some(2),
                duration_ms: 200_000,
                title: title.into(),
                album_id: album,
                disc_no: None,
                track_no: Some(no),
                year: None,
                identity: if album.is_some() {
                    Identity::Inferred
                } else {
                    Identity::Unknown
                },
                identity_conf: 0.5,
                unknown_reason: None,
                artists,
            },
        )
        .unwrap()
    }

    #[test]
    fn collection_pages_counts_and_search_follow_edits() {
        let mut c = open_in_memory().unwrap();
        let bi2 = artist_get_or_create(&c, "Би-2").unwrap();
        let zem = artist_get_or_create(&c, "Земфира").unwrap();
        let _unused = artist_get_or_create(&c, "Без треков").unwrap();
        let ino = album_get_or_create(&c, "Иномарки", bi2, Some(2004)).unwrap();
        let ven = album_get_or_create(&c, "Вендетта", zem, Some(2005)).unwrap();
        add(&mut c, "Варвара", vec![bi2, zem], Some(ino), 2);
        add(&mut c, "Мой рок-н-ролл", vec![bi2], Some(ino), 1);
        let isk = add(&mut c, "Искала", vec![zem], Some(ven), 3);
        add(&mut c, "Track 01", vec![zem], None, 1);

        let (artists, next) = artists_page(&c, None, 100).unwrap();
        assert_eq!(
            artists
                .iter()
                .map(|a| (a.name.as_str(), a.tracks))
                .collect::<Vec<_>>(),
            [("Би-2", 2), ("Земфира", 3)]
        );
        assert!(next.is_none());
        let (a1, next) = albums_page(&c, None, 1).unwrap();
        assert_eq!(a1[0].title, "Вендетта");
        let (a2, _) = albums_page(&c, next.as_ref(), 1).unwrap();
        assert_eq!(
            (a2[0].title.as_str(), a2[0].artist.as_str(), a2[0].tracks),
            ("Иномарки", "Би-2", 2)
        );
        let order: Vec<_> = album_tracks(&c, ino)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(order, ["Мой рок-н-ролл", "Варвара"]);
        assert_eq!(artist_tracks(&c, zem).unwrap().len(), 3);
        let n = counts(&c).unwrap();
        assert_eq!((n.tracks, n.artists, n.albums, n.unknown), (4, 2, 2, 1));
        assert_eq!(n.duration_ms, 800_000);
        let unknown: Vec<_> = unknown_tracks(&c, 100)
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        assert_eq!(unknown, ["Track 01"]);

        // T1.10: search follows edits and removals.
        let hits = |c: &Connection, q: &str| -> i64 {
            c.query_row(
                "SELECT count(*) FROM search_fts WHERE search_fts MATCH ?1",
                [q],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(hits(&c, "искала"), 1);
        track_edit(
            &mut c,
            isk,
            &TrackEdit {
                title: "Хочешь".into(),
                album_id: Some(ven),
                artists: vec![zem],
                genre_main: Some("rock".into()),
                identity: Identity::User,
            },
        )
        .unwrap();
        assert_eq!(hits(&c, "искала"), 0);
        assert_eq!(hits(&c, "хочешь"), 1);
        assert_eq!(hits(&c, "genre:rock"), 1);
        track_remove(&mut c, isk).unwrap();
        assert_eq!(hits(&c, "хочешь"), 0);
        assert_eq!(counts(&c).unwrap().tracks, 3);
    }
}
