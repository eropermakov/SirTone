//! Lexical search (T2.13 + T3.15, FR-SRCH-001…003; `05` §9 steps 1–2, `01b` F-17; S-15: p95
//! 16.6 ms on 100 000 tracks). The query in several forms (`search_text`: as typed, the keyboard
//! layout fixed, transliterated) → FTS5 prefix tokens OR their Snowball stems, bm25 (title 4,
//! artists 5, album 2, genre 1, aliases 1); the forms merge by score × form weight, boosted for
//! an exact title (×2), a title starting with the first word (×1.5) and `log(1+plays)`. Fewer
//! than 5 hits → spelling corrected on the index vocabulary (Damerau–Levenshtein ≤ 2 for long
//! words, ≤ 1 for short; D-82). Tiers stop once there are 5 hits, so a query typed right costs
//! one FTS lookup. Artists and albums by their normalised names in any form, playlists by
//! name; «лучшее совпадение» first (Spotify / Apple Music).

use rusqlite::{Connection, OptionalExtension, params};
use sirtone_core::text::norm;

use crate::collection::{AlbumRow, ArtistRow, TRACK_COLS, track_row};
use crate::error::LibraryError;
use crate::repo::{TrackId, TrackRow};
use crate::search_text::{Form, forms, fts_query};

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

/// How close a query word is to some word of a text (also as a prefix: «аривед» ~ «ариведерчи»).
fn word_distance(q: &str, words: &[&str]) -> usize {
    let n = q.chars().count();
    words
        .iter()
        .map(|w| {
            let full = dl(q, w);
            let prefix: String = w.chars().take(n).collect();
            full.min(dl(q, &prefix) + usize::from(prefix.chars().count() < n))
        })
        .min()
        .unwrap_or(usize::MAX)
}

/// Typos (spell correction on the index vocabulary, like SymSpell/Lucene suggesters): each
/// query word of 4+ letters → indexed words of a close length with the same first letter within
/// Damerau–Levenshtein ≤ 2 (≥ 5 letters) / ≤ 1; the corrected words are searched exactly.
/// Shorter words stay prefixes. `None` when nothing could be corrected.
fn corrected_query(conn: &Connection, text: &str) -> Result<Option<String>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT term FROM search_vocab WHERE term >= ?1 AND term < ?2
           AND length(term) BETWEEN ?3 AND ?4",
    )?;
    let mut parts = Vec::new();
    let mut changed = false;
    for w in text.split_whitespace() {
        let w = w.replace('"', "");
        let n = w.chars().count();
        if n < 4 {
            parts.push(format!("\"{w}\"*"));
            continue;
        }
        let Some(first) = w.chars().next() else {
            continue;
        };
        // «е», «э», «ё» stand for each other at a word start (transliteration: eksponat).
        let firsts: &[char] = match first {
            'е' | 'э' | 'ё' => &['е', 'э', 'ё'],
            _ => std::slice::from_ref(&first),
        };
        let limit = if n >= 5 { 2 } else { 1 };
        let mut near: Vec<(usize, String)> = Vec::new();
        for &c in firsts {
            let lo = c.to_string();
            let hi = char::from_u32(c as u32 + 1).map_or_else(|| lo.clone(), |c| c.to_string());
            let found: Vec<String> = st
                .query_map(params![lo, hi, n as i64 - 2, n as i64 + 2], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            near.extend(found.into_iter().filter_map(|t| {
                let d = word_distance(&w, &[t.as_str()]);
                (d <= limit).then_some((d, t))
            }));
        }
        near.sort();
        near.truncate(5);
        if near.is_empty() {
            parts.push(format!("\"{w}\"*"));
        } else {
            changed |= near.iter().any(|(d, _)| *d > 0);
            let alts: Vec<String> = near.iter().map(|(_, t)| format!("\"{t}\"")).collect();
            parts.push(format!("({})", alts.join(" OR ")));
        }
    }
    Ok((changed && !parts.is_empty()).then(|| parts.join(" AND ")))
}

/// Runs one FTS5 query and merges its hits into `scored` (score × form weight × boosts).
fn rank(
    st: &mut rusqlite::CachedStatement<'_>,
    q: &str,
    f: &Form,
    scored: &mut Vec<(TrackId, f64)>,
) -> Result<bool, LibraryError> {
    let first = f.text.split_whitespace().next().unwrap_or("");
    let rows: Vec<(TrackId, f64, String, i64)> = st
        .query_map([q], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let hit = !rows.is_empty();
    for (id, bm, title, plays) in rows {
        let mut score = -bm * f.weight;
        if title == f.text {
            score *= 2.0;
        } else if title.starts_with(first) {
            score *= 1.5;
        }
        score *= 1.0 + 0.1 * (1.0 + plays.max(0) as f64).ln();
        match scored.iter_mut().find(|(x, _)| *x == id) {
            Some(e) => e.1 = e.1.max(score),
            None => scored.push((id, score)),
        }
    }
    Ok(hit)
}

/// Tracks for the query, in tiers that stop once there are 5 hits: (1) as typed, word
/// prefixes; (2) every form (layout, transliteration) with stems and «ё»; (3) spelling
/// corrected. Returns the ids and the forms worth looking up artists and albums with (the one
/// as typed, and those that found tracks).
fn track_ids(conn: &Connection, forms: &[Form]) -> Result<(Vec<TrackId>, Vec<Form>), LibraryError> {
    let mut scored: Vec<(TrackId, f64)> = Vec::new();
    let mut st = conn.prepare_cached(
        // Ranked inside FTS first, the track table joined for the top 200 only (the index holds
        // playable tracks only — `search_reindex`).
        "SELECT x.rowid, x.score, t.title_norm, t.play_count
         FROM (SELECT rowid, bm25(search_fts, 4.0, 5.0, 2.0, 1.0, 1.0) AS score FROM search_fts
               WHERE search_fts MATCH ?1 ORDER BY score LIMIT 200) x
         JOIN track t ON t.id = x.rowid WHERE t.state = 'ok'",
    )?;
    let mut used: Vec<Form> = forms.first().cloned().into_iter().collect();
    if let Some(f) = forms.first()
        && let Some(q) = fts_query(&f.text, false)
    {
        rank(&mut st, &q, f, &mut scored)?;
    }
    if scored.len() < 5 {
        for (i, f) in forms.iter().enumerate() {
            if let Some(q) = fts_query(&f.text, true)
                && rank(&mut st, &q, f, &mut scored)?
                && i > 0
            {
                used.push(f.clone());
            }
        }
    }
    if scored.len() < 5 {
        for f in forms {
            if let Some(q) = corrected_query(conn, &f.text)? {
                let typo = Form {
                    text: f.text.clone(),
                    weight: f.weight * 0.8,
                };
                rank(&mut st, &q, &typo, &mut scored)?;
                if !used.iter().any(|u| u.text == f.text) {
                    used.push(f.clone());
                }
            }
        }
    }
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut ids: Vec<TrackId> = scored.into_iter().map(|(id, _)| id).collect();
    ids.truncate(TRACKS);
    Ok((ids, used))
}

pub fn search(conn: &Connection, query: &str) -> Result<Results, LibraryError> {
    let Some(qn) = norm(query).filter(|q| !q.trim().is_empty()) else {
        return Ok(Results::default());
    };
    let qn = qn.trim().to_string();
    let forms = forms(query);

    let mut get = conn.prepare_cached(&format!(
        "SELECT {TRACK_COLS} FROM track t LEFT JOIN album al ON al.id = t.album_id WHERE t.id = ?1"
    ))?;
    let mut tracks = Vec::new();
    let (ids, used) = track_ids(conn, &forms)?;
    // Artists and albums: the query as typed and the forms that found tracks — plus every form
    // when nothing was found (an artist without a matching title still shows up).
    let forms: &[Form] = if ids.is_empty() { &forms } else { &used };
    for id in ids {
        tracks.push(get.query_row([id], track_row)?);
    }

    let mut st = conn.prepare_cached(
        // Candidates by name first (no counting over every match), then their track counts.
        "SELECT c.id, c.name, c.name_norm,
                (SELECT count(*) FROM track_artist ta JOIN track t ON t.id = ta.track_id
                 WHERE ta.artist_id = c.id AND t.state = 'ok') AS n
         FROM (SELECT a.id, a.name, a.name_norm FROM artist a
               WHERE a.merged_into IS NULL
                 AND (a.name_norm LIKE ?1 ESCAPE '\\' OR a.name_norm LIKE ?2 ESCAPE '\\')
               ORDER BY a.name_norm = ?3 DESC, a.name_norm LIKE ?1 ESCAPE '\\' DESC LIMIT 60) c
         WHERE n > 0
         ORDER BY c.name_norm = ?3 DESC, c.name_norm LIKE ?1 ESCAPE '\\' DESC, n DESC LIMIT ?4",
    )?;
    let mut artists: Vec<(ArtistRow, bool)> = Vec::new();
    for f in forms {
        let prefix = format!("{}%", like_escape(&f.text));
        let word = format!("% {}%", like_escape(&f.text));
        let rows: Vec<(ArtistRow, bool)> = st
            .query_map(params![prefix, word, f.text, GROUP], |r| {
                Ok((
                    ArtistRow {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        tracks: r.get(3)?,
                        added_at: 0,
                    },
                    r.get::<_, String>(2)? == f.text,
                ))
            })?
            .collect::<Result<_, _>>()?;
        for (a, exact) in rows {
            match artists.iter_mut().find(|(x, _)| x.id == a.id) {
                Some(e) => e.1 |= exact,
                None => artists.push((a, exact)),
            }
        }
    }
    artists.truncate(GROUP as usize);

    let mut st = conn.prepare_cached(
        "SELECT c.id, c.title, a.name, c.year, c.cover_id, cv.hash, c.title_norm,
                (SELECT count(*) FROM track t WHERE t.album_id = c.id AND t.state = 'ok') AS n
         FROM (SELECT al.id, al.title, al.year, al.cover_id, al.title_norm, al.album_artist_id
               FROM album al
               WHERE al.title_norm LIKE ?1 ESCAPE '\\' OR al.title_norm LIKE ?2 ESCAPE '\\'
               ORDER BY al.title_norm = ?3 DESC, al.title_norm LIKE ?1 ESCAPE '\\' DESC LIMIT 60) c
         JOIN artist a ON a.id = c.album_artist_id
         LEFT JOIN cover cv ON cv.id = c.cover_id
         WHERE n > 0
         ORDER BY c.title_norm = ?3 DESC, c.title_norm LIKE ?1 ESCAPE '\\' DESC, n DESC LIMIT ?4",
    )?;
    let mut albums: Vec<(AlbumRow, bool)> = Vec::new();
    for f in forms {
        let prefix = format!("{}%", like_escape(&f.text));
        let word = format!("% {}%", like_escape(&f.text));
        let rows: Vec<(AlbumRow, bool)> = st
            .query_map(params![prefix, word, f.text, GROUP], |r| {
                Ok((
                    AlbumRow {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        artist: r.get(2)?,
                        year: r.get(3)?,
                        cover_id: r.get(4)?,
                        cover: r.get(5)?,
                        tracks: r.get(7)?,
                        added_at: 0,
                    },
                    r.get::<_, String>(6)? == f.text,
                ))
            })?
            .collect::<Result<_, _>>()?;
        for (a, exact) in rows {
            match albums.iter_mut().find(|(x, _)| x.id == a.id) {
                Some(e) => e.1 |= exact,
                None => albums.push((a, exact)),
            }
        }
    }
    albums.truncate(GROUP as usize);

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

    // An artist name typed loosely (transliterated without «ь», a typo): the artists of the
    // top tracks whose every word is within one edit of the query's words count as exact.
    if !artists.iter().any(|(_, exact)| *exact) {
        let mut st = conn.prepare_cached(
            "SELECT a.id, a.name, a.name_norm,
                    (SELECT count(*) FROM track_artist x JOIN track t ON t.id = x.track_id
                     WHERE x.artist_id = a.id AND t.state = 'ok')
             FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
             WHERE ta.track_id = ?1 ORDER BY ta.position LIMIT 1",
        )?;
        'tracks: for t in tracks.iter().take(5) {
            let Some((id, name, name_norm, n)): Option<(i64, String, String, u32)> = st
                .query_row([t.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .optional()?
            else {
                continue;
            };
            let aw: Vec<&str> = name_norm.split_whitespace().collect();
            for f in forms {
                let qw: Vec<&str> = f.text.split_whitespace().collect();
                let close = aw.len() == qw.len()
                    && aw.iter().zip(&qw).all(|(a, q)| {
                        let limit = if a.chars().count() >= 4 { 1 } else { 0 };
                        dl(a, q) <= limit
                    });
                if close {
                    artists.retain(|(x, _)| x.id != id);
                    artists.insert(
                        0,
                        (
                            ArtistRow {
                                id,
                                name: name.clone(),
                                tracks: n,
                                added_at: 0,
                            },
                            true,
                        ),
                    );
                    break 'tracks;
                }
            }
        }
    }

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
