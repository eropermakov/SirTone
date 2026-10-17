//! Lexical search (T2.13, FR-SRCH-001/002; `05` §9 steps 2, `01b` F-17; measured in S-15:
//! p95 16.6 ms on 100 000 tracks): FTS5 prefix tokens with bm25 weights (title 4, artists 5,
//! album 2, genre 1, aliases 1); fewer than 5 hits → trigrams re-ranked by Damerau–Levenshtein
//! ≤ 2. Artists and albums by their normalised names, playlists by name; «лучшее совпадение»
//! first (Spotify / Apple Music). Layout fix, transliteration and stemming come with T3.15.

use rusqlite::{Connection, params};
use sirtone_core::text::norm;

use crate::collection::{AlbumRow, ArtistRow, TRACK_COLS, track_row};
use crate::error::LibraryError;
use crate::repo::{TrackId, TrackRow};

/// The top result.
#[derive(Debug, Clone, PartialEq)]
pub enum Best {
    Track(TrackRow),
    Artist(ArtistRow),
    Album(AlbumRow),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Results {
    pub best: Option<Best>,
    pub tracks: Vec<TrackRow>,
    pub artists: Vec<ArtistRow>,
    pub albums: Vec<AlbumRow>,
    /// (id, name).
    pub playlists: Vec<(i64, String)>,
}

const TRACKS: usize = 50;
const GROUP: u32 = 8;

/// `LIKE` pattern pieces: `%`, `_` and `\` are literal.
fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn dl(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

fn track_ids(conn: &Connection, qn: &str) -> Result<Vec<TrackId>, LibraryError> {
    let tokens: Vec<String> = qn
        .split_whitespace()
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect();
    let mut ids: Vec<TrackId> = Vec::new();
    if !tokens.is_empty() {
        let mut st = conn.prepare_cached(
            "SELECT s.rowid FROM search_fts s JOIN track t ON t.id = s.rowid
             WHERE search_fts MATCH ?1 AND t.state = 'ok'
             ORDER BY bm25(search_fts, 4.0, 5.0, 2.0, 1.0, 1.0) LIMIT 200",
        )?;
        ids = st
            .query_map([tokens.join(" ")], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
    }
    if ids.len() < 5 {
        // Typos: any trigram of the query, then Damerau–Levenshtein ≤ 2 on words ≥ 5 letters.
        let chars: Vec<char> = qn.chars().collect();
        let mut grams: Vec<String> = chars
            .windows(3)
            .map(|w| w.iter().collect::<String>())
            .filter(|g| !g.contains(' '))
            .collect();
        grams.sort();
        grams.dedup();
        let qtok: Vec<&str> = qn
            .split_whitespace()
            .filter(|t| t.chars().count() >= 5)
            .collect();
        if !grams.is_empty() && !qtok.is_empty() {
            let expr = grams
                .iter()
                .map(|g| format!("\"{g}\""))
                .collect::<Vec<_>>()
                .join(" OR ");
            let mut st = conn.prepare_cached(
                "SELECT rowid FROM search_tri WHERE search_tri MATCH ?1 ORDER BY rank LIMIT 200",
            )?;
            let cand: Vec<TrackId> = st
                .query_map([expr], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            let mut text = conn.prepare_cached(
                "SELECT t.title_norm || ' ' || coalesce((SELECT group_concat(a.name_norm, ' ')
                   FROM track_artist ta JOIN artist a ON a.id = ta.artist_id WHERE ta.track_id = t.id), '')
                 FROM track t WHERE t.id = ?1 AND t.state = 'ok'",
            )?;
            let mut scored: Vec<(TrackId, usize)> = Vec::new();
            for id in cand {
                if ids.contains(&id) {
                    continue;
                }
                let Ok(t) = text.query_row([id], |r| r.get::<_, String>(0)) else {
                    continue;
                };
                let d = qtok
                    .iter()
                    .map(|q| t.split_whitespace().map(|w| dl(q, w)).min().unwrap_or(9))
                    .min()
                    .unwrap_or(9);
                if d <= 2 {
                    scored.push((id, d));
                }
            }
            scored.sort_by_key(|&(_, d)| d);
            ids.extend(scored.into_iter().map(|(id, _)| id));
        }
    }
    ids.truncate(TRACKS);
    Ok(ids)
}

pub fn search(conn: &Connection, query: &str) -> Result<Results, LibraryError> {
    let Some(qn) = norm(query).filter(|q| !q.trim().is_empty()) else {
        return Ok(Results::default());
    };
    let qn = qn.trim().to_string();
    let prefix = format!("{}%", like_escape(&qn));
    let word = format!("% {}%", like_escape(&qn));

    let mut get = conn.prepare_cached(&format!(
        "SELECT {TRACK_COLS} FROM track t LEFT JOIN album al ON al.id = t.album_id WHERE t.id = ?1"
    ))?;
    let mut tracks = Vec::new();
    for id in track_ids(conn, &qn)? {
        tracks.push(get.query_row([id], track_row)?);
    }

    let mut st = conn.prepare_cached(
        "SELECT a.id, a.name, a.name_norm,
                (SELECT count(*) FROM track_artist ta JOIN track t ON t.id = ta.track_id
                 WHERE ta.artist_id = a.id AND t.state = 'ok') AS n
         FROM artist a
         WHERE a.merged_into IS NULL AND n > 0
           AND (a.name_norm LIKE ?1 ESCAPE '\\' OR a.name_norm LIKE ?2 ESCAPE '\\')
         ORDER BY a.name_norm = ?3 DESC, a.name_norm LIKE ?1 ESCAPE '\\' DESC, n DESC LIMIT ?4",
    )?;
    let mut artists: Vec<(ArtistRow, bool)> = st
        .query_map(params![prefix, word, qn, GROUP], |r| {
            Ok((
                ArtistRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    tracks: r.get(3)?,
                },
                r.get::<_, String>(2)? == qn,
            ))
        })?
        .collect::<Result<_, _>>()?;

    let mut st = conn.prepare_cached(
        "SELECT al.id, al.title, a.name, al.year, al.cover_id, cv.hash, al.title_norm,
                (SELECT count(*) FROM track t WHERE t.album_id = al.id AND t.state = 'ok') AS n
         FROM album al JOIN artist a ON a.id = al.album_artist_id
         LEFT JOIN cover cv ON cv.id = al.cover_id
         WHERE n > 0 AND (al.title_norm LIKE ?1 ESCAPE '\\' OR al.title_norm LIKE ?2 ESCAPE '\\')
         ORDER BY al.title_norm = ?3 DESC, al.title_norm LIKE ?1 ESCAPE '\\' DESC, n DESC LIMIT ?4",
    )?;
    let mut albums: Vec<(AlbumRow, bool)> = st
        .query_map(params![prefix, word, qn, GROUP], |r| {
            Ok((
                AlbumRow {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    year: r.get(3)?,
                    cover_id: r.get(4)?,
                    cover: r.get(5)?,
                    tracks: r.get(7)?,
                },
                r.get::<_, String>(6)? == qn,
            ))
        })?
        .collect::<Result<_, _>>()?;

    let mut st = conn.prepare_cached(
        "SELECT id, name FROM playlist WHERE kind = 'manual' ORDER BY sort_order, id",
    )?;
    let playlists: Vec<(i64, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|(_, name): &(i64, String)| norm(name).is_some_and(|n| n.contains(&qn)))
        .take(GROUP as usize)
        .collect();

    // «Лучшее совпадение»: the exact artist, else the exact album, else the first track.
    let best = if let Some(i) = artists.iter().position(|(_, exact)| *exact) {
        Some(Best::Artist(artists.remove(i).0))
    } else if let Some(i) = albums.iter().position(|(_, exact)| *exact) {
        Some(Best::Album(albums.remove(i).0))
    } else if !tracks.is_empty() {
        Some(Best::Track(tracks.remove(0)))
    } else if !artists.is_empty() {
        Some(Best::Artist(artists.remove(0).0))
    } else {
        None
    };

    Ok(Results {
        best,
        tracks,
        artists: artists.into_iter().map(|(a, _)| a).collect(),
        albums: albums.into_iter().map(|(a, _)| a).collect(),
        playlists,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open;
    use crate::repo::{
        Identity, NewTrack, album_get_or_create, artist_get_or_create, track_insert,
    };

    #[test]
    fn finds_by_prefix_typo_artist_and_album() {
        let root = std::env::temp_dir().join(format!("sirtone-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut c = open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap();
        let kino = artist_get_or_create(&c, "Кино").unwrap();
        let zemfira = artist_get_or_create(&c, "Земфира").unwrap();
        let album = album_get_or_create(&c, "Группа крови", kino, Some(1988)).unwrap();
        for (n, (title, artist, al)) in [
            ("Группа крови", kino, Some(album)),
            ("Звезда по имени Солнце", kino, None),
            ("Полковнику никто не пишет", zemfira, None),
            ("Искала", zemfira, None),
        ]
        .into_iter()
        .enumerate()
        {
            track_insert(
                &mut c,
                &NewTrack {
                    rel_path: format!("x/{n}.mp3"),
                    size_bytes: 5,
                    mtime_ms: 0,
                    content_hash: format!("h{n}"),
                    container: "mp3".into(),
                    codec: "mp3".into(),
                    lossless: false,
                    sample_rate: None,
                    channels: None,
                    duration_ms: 1000,
                    title: title.into(),
                    album_id: al,
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
        }
        // Prefix of a title word.
        let r = search(&c, "звез").unwrap();
        assert!(matches!(&r.best, Some(Best::Track(t)) if t.title == "Звезда по имени Солнце"));
        // The exact artist wins «лучшее совпадение»; the tracks follow.
        let r = search(&c, "кино").unwrap();
        assert!(matches!(&r.best, Some(Best::Artist(a)) if a.name == "Кино"));
        assert_eq!(r.tracks.len(), 2);
        // An exact album.
        let r = search(&c, "группа крови").unwrap();
        assert!(matches!(&r.best, Some(Best::Album(a)) if a.title == "Группа крови"));
        // A typo (swapped letters) through trigrams.
        let r = search(&c, "плоковнику").unwrap();
        assert!(matches!(&r.best, Some(Best::Track(t)) if t.title.starts_with("Полковнику")));
        // ё/е and case; LIKE wildcards are literal.
        assert!(search(&c, "ЗЕМФИРА").unwrap().best.is_some());
        assert!(search(&c, "%").unwrap().best.is_none());
        assert_eq!(search(&c, "   ").unwrap(), Results::default());
        let _ = std::fs::remove_dir_all(&root);
    }
}
