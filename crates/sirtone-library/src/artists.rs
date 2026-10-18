//! Artist merge (T3.12, FR-ART-003/004, AT-06; `05` §5, `01b` F-10 — Plex/Roon merge,
//! MusicBrainz aliases). Equal normalised names merge on import already («Би-2» = «Би 2»);
//! here: names that differ only by script or a leading «The» are **suggested**, and any two
//! artists can be merged by hand. A merge, in one transaction: the dropped artist's track
//! links and albums go to the kept one (albums with the same title join), its ♡ / «никогда»
//! follow, its spelling becomes an alias (a later import under it lands with the kept artist),
//! and the row stays as `merged_into` — so the merge can be undone in the session from an exact
//! record of what changed. Files are put in place afterwards, track by track
//! (`relocate::relayout`).

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::LibraryError;
use crate::repo::{self, ArtistId, TrackId};
use crate::search_text::cyr_to_lat;

/// What a link of the dropped artist became.
#[derive(Debug, Clone, PartialEq)]
enum Link {
    /// `artist_id` changed to the kept artist.
    Repointed,
    /// The track already had the kept artist: the row was removed.
    Deleted,
}

/// Everything a merge changed, to undo exactly that (FR-ART-004 «отмена в течение сессии»).
#[derive(Debug, Clone, PartialEq)]
pub struct MergeRecord {
    pub keep: ArtistId,
    pub drop: ArtistId,
    /// Tracks whose files may need to move (links or albums changed).
    pub tracks: Vec<TrackId>,
    links: Vec<(TrackId, String, i64, Link)>,
    /// The kept artist's links that became primary: (track, old role, old position).
    promoted: Vec<(TrackId, String, i64)>,
    /// Albums whose album artist changed to the kept one.
    albums: Vec<i64>,
    /// Tracks moved into the kept artist's album of the same title: (track, old album).
    album_moves: Vec<(TrackId, i64)>,
    /// A kept album that took the dropped one's cover: (album, old cover).
    covers: Vec<(i64, Option<i64>)>,
    feedback: Vec<i64>,
    keep_never: i64,
    alias_norm: String,
    /// The alias row the merge replaced: (artist, source).
    alias_prev: Option<(ArtistId, String)>,
    /// Aliases of the dropped artist moved to the kept one.
    aliases: Vec<String>,
}

fn artist(conn: &Connection, id: ArtistId) -> Result<(String, String, i64), LibraryError> {
    conn.query_row(
        "SELECT name, name_norm, wave_never FROM artist WHERE id = ?1 AND merged_into IS NULL",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()?
    .ok_or_else(|| LibraryError::Invalid(format!("no artist {id}")))
}

/// Merges `drop` into `keep` (both canonical) in the database; returns what changed.
pub fn merge(
    conn: &mut Connection,
    keep: ArtistId,
    drop: ArtistId,
) -> Result<MergeRecord, LibraryError> {
    if keep == drop {
        return Err(LibraryError::Invalid(
            "an artist cannot merge into itself".into(),
        ));
    }
    let tx = conn.transaction()?;
    let (_, _, keep_never) = artist(&tx, keep)?;
    let (_, drop_norm, drop_never) = artist(&tx, drop)?;
    let mut rec = MergeRecord {
        keep,
        drop,
        tracks: Vec::new(),
        links: Vec::new(),
        promoted: Vec::new(),
        albums: Vec::new(),
        album_moves: Vec::new(),
        covers: Vec::new(),
        feedback: Vec::new(),
        keep_never,
        alias_norm: drop_norm.clone(),
        alias_prev: None,
        aliases: Vec::new(),
    };
    // Track links. The primary key includes the role: a track with both spellings keeps one link.
    let links: Vec<(TrackId, String, i64)> = {
        let mut st =
            tx.prepare("SELECT track_id, role, position FROM track_artist WHERE artist_id = ?1")?;
        st.query_map([drop], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?
    };
    for (track, role, position) in links {
        let kept: Option<(String, i64)> = tx
            .query_row(
                "SELECT role, position FROM track_artist WHERE track_id = ?1 AND artist_id = ?2
                 ORDER BY role = 'primary' DESC LIMIT 1",
                params![track, keep],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match kept {
            Some((krole, kpos)) => {
                tx.execute(
                    "DELETE FROM track_artist WHERE track_id = ?1 AND artist_id = ?2 AND role = ?3",
                    params![track, drop, role],
                )?;
                if role == "primary" && krole != "primary" {
                    tx.execute(
                        "UPDATE track_artist SET role = 'primary', position = ?4
                         WHERE track_id = ?1 AND artist_id = ?2 AND role = ?3",
                        params![track, keep, krole, position],
                    )?;
                    rec.promoted.push((track, krole, kpos));
                }
                rec.links.push((track, role, position, Link::Deleted));
            }
            None => {
                tx.execute(
                    "UPDATE track_artist SET artist_id = ?3 WHERE track_id = ?1 AND artist_id = ?2 AND role = ?4",
                    params![track, drop, keep, role],
                )?;
                rec.links.push((track, role, position, Link::Repointed));
            }
        }
        if !rec.tracks.contains(&track) {
            rec.tracks.push(track);
        }
    }
    // Albums: the same title under the kept artist joins it; others change their album artist.
    let albums: Vec<(i64, String, Option<i64>)> = {
        let mut st =
            tx.prepare("SELECT id, title_norm, cover_id FROM album WHERE album_artist_id = ?1")?;
        st.query_map([drop], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?
    };
    for (album, title, cover) in albums {
        let twin: Option<(i64, Option<i64>)> = tx
            .query_row(
                "SELECT id, cover_id FROM album WHERE album_artist_id = ?1 AND title_norm = ?2
                 ORDER BY id LIMIT 1",
                params![keep, title],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let moved: Vec<TrackId> = {
            let mut st = tx.prepare("SELECT id FROM track WHERE album_id = ?1")?;
            st.query_map([album], |r| r.get(0))?
                .collect::<Result<_, _>>()?
        };
        match twin {
            Some((k, kcover)) => {
                tx.execute(
                    "UPDATE track SET album_id = ?2 WHERE album_id = ?1",
                    params![album, k],
                )?;
                rec.album_moves.extend(moved.iter().map(|&t| (t, album)));
                if kcover.is_none() && cover.is_some() {
                    tx.execute(
                        "UPDATE album SET cover_id = ?2 WHERE id = ?1",
                        params![k, cover],
                    )?;
                    rec.covers.push((k, None));
                }
            }
            None => {
                tx.execute(
                    "UPDATE album SET album_artist_id = ?2 WHERE id = ?1",
                    params![album, keep],
                )?;
                rec.albums.push(album);
            }
        }
        for t in moved {
            if !rec.tracks.contains(&t) {
                rec.tracks.push(t);
            }
        }
    }
    // ♡ / «никогда» for the artist.
    rec.feedback = {
        let mut st = tx.prepare("SELECT id FROM feedback WHERE artist_id = ?1")?;
        st.query_map([drop], |r| r.get(0))?
            .collect::<Result<_, _>>()?
    };
    tx.execute(
        "UPDATE feedback SET artist_id = ?2 WHERE artist_id = ?1",
        params![drop, keep],
    )?;
    tx.execute(
        "UPDATE artist SET wave_never = ?2 WHERE id = ?1",
        params![keep, i64::from(keep_never != 0 || drop_never != 0)],
    )?;
    // The spelling becomes an alias; the dropped artist's own aliases follow.
    rec.aliases = {
        let mut st = tx.prepare("SELECT alias_norm FROM artist_alias WHERE artist_id = ?1")?;
        st.query_map([drop], |r| r.get(0))?
            .collect::<Result<_, _>>()?
    };
    tx.execute(
        "UPDATE artist_alias SET artist_id = ?2 WHERE artist_id = ?1",
        params![drop, keep],
    )?;
    rec.alias_prev = tx
        .query_row(
            "SELECT artist_id, source FROM artist_alias WHERE alias_norm = ?1",
            [&drop_norm],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    tx.execute(
        "INSERT OR REPLACE INTO artist_alias(alias_norm, artist_id, source) VALUES (?1, ?2, 'merge')",
        params![drop_norm, keep],
    )?;
    tx.execute(
        "UPDATE artist SET merged_into = ?2 WHERE id = ?1",
        params![drop, keep],
    )?;
    for &t in &rec.tracks {
        repo::search_reindex(&tx, t)?;
    }
    tx.commit()?;
    Ok(rec)
}

/// Undoes a merge exactly; refused when the library changed under it (a track gone, the dropped
/// artist used again). Returns the tracks whose files need to go back.
pub fn undo(conn: &mut Connection, rec: &MergeRecord) -> Result<Vec<TrackId>, LibraryError> {
    let tx = conn.transaction()?;
    let still: Option<ArtistId> = tx
        .query_row(
            "SELECT merged_into FROM artist WHERE id = ?1",
            [rec.drop],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if still != Some(rec.keep) {
        return Err(LibraryError::Invalid("the merge was changed since".into()));
    }
    for &t in &rec.tracks {
        let alive: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM track WHERE id = ?1)",
            [t],
            |r| r.get(0),
        )?;
        if !alive {
            return Err(LibraryError::Invalid(format!(
                "track {t} is gone since the merge"
            )));
        }
    }
    tx.execute(
        "UPDATE artist SET merged_into = NULL WHERE id = ?1",
        [rec.drop],
    )?;
    match &rec.alias_prev {
        Some((a, source)) => tx.execute(
            "UPDATE artist_alias SET artist_id = ?2, source = ?3 WHERE alias_norm = ?1",
            params![rec.alias_norm, a, source],
        )?,
        None => tx.execute(
            "DELETE FROM artist_alias WHERE alias_norm = ?1",
            [&rec.alias_norm],
        )?,
    };
    for a in &rec.aliases {
        tx.execute(
            "UPDATE artist_alias SET artist_id = ?2 WHERE alias_norm = ?1",
            params![a, rec.drop],
        )?;
    }
    tx.execute(
        "UPDATE artist SET wave_never = ?2 WHERE id = ?1",
        params![rec.keep, rec.keep_never],
    )?;
    for id in &rec.feedback {
        tx.execute(
            "UPDATE feedback SET artist_id = ?2 WHERE id = ?1",
            params![id, rec.drop],
        )?;
    }
    for (album, cover) in &rec.covers {
        tx.execute(
            "UPDATE album SET cover_id = ?2 WHERE id = ?1",
            params![album, cover],
        )?;
    }
    for (track, album) in &rec.album_moves {
        tx.execute(
            "UPDATE track SET album_id = ?2 WHERE id = ?1",
            params![track, album],
        )?;
    }
    for album in &rec.albums {
        tx.execute(
            "UPDATE album SET album_artist_id = ?2 WHERE id = ?1",
            params![album, rec.drop],
        )?;
    }
    for (track, role, pos) in &rec.promoted {
        tx.execute(
            "UPDATE track_artist SET role = ?3, position = ?4
             WHERE track_id = ?1 AND artist_id = ?2 AND role = 'primary'",
            params![track, rec.keep, role, pos],
        )?;
    }
    for (track, role, pos, link) in &rec.links {
        match link {
            Link::Repointed => tx.execute(
                "UPDATE track_artist SET artist_id = ?2 WHERE track_id = ?1 AND artist_id = ?3 AND role = ?4",
                params![track, rec.drop, rec.keep, role],
            )?,
            Link::Deleted => tx.execute(
                "INSERT OR IGNORE INTO track_artist(track_id, artist_id, role, position)
                 VALUES (?1, ?2, ?3, ?4)",
                params![track, rec.drop, role, pos],
            )?,
        };
    }
    for &t in &rec.tracks {
        repo::search_reindex(&tx, t)?;
    }
    tx.commit()?;
    Ok(rec.tracks.clone())
}

/// A suggested merge: two spellings of one artist.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    /// The one with more tracks (kept by default).
    pub keep: (ArtistId, String, u32),
    pub drop: (ArtistId, String, u32),
}

/// The comparison key: Latin letters (transliterated), no leading «the», no spaces.
fn key(name_norm: &str) -> String {
    let lat = cyr_to_lat(name_norm);
    let lat = lat.strip_prefix("the ").unwrap_or(&lat);
    lat.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Pairs that look like one artist (FR-ART-003 «предложение по транслитерации/схожести»):
/// the same key after transliteration and without «The». Turned-down pairs are not offered.
pub fn suggestions(conn: &Connection) -> Result<Vec<Suggestion>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT a.id, a.name, a.name_norm,
                (SELECT count(*) FROM track_artist ta JOIN track t ON t.id = ta.track_id
                 WHERE ta.artist_id = a.id AND t.state = 'ok') AS n
         FROM artist a WHERE a.merged_into IS NULL AND a.kind = 'artist' AND n > 0",
    )?;
    let rows: Vec<(ArtistId, String, String, u32)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let mut groups: std::collections::HashMap<String, Vec<(ArtistId, String, u32)>> =
        std::collections::HashMap::new();
    for (id, name, norm, n) in rows {
        let k = key(&norm);
        if k.chars().count() >= 2 {
            groups.entry(k).or_default().push((id, name, n));
        }
    }
    let mut dismissed = conn.prepare_cached(
        "SELECT EXISTS (SELECT 1 FROM artist_pair_dismissed WHERE a = ?1 AND b = ?2)",
    )?;
    let mut out = Vec::new();
    for mut g in groups.into_values().filter(|g| g.len() > 1) {
        g.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        let keep = g[0].clone();
        for d in g.into_iter().skip(1) {
            let (a, b) = if keep.0 < d.0 {
                (keep.0, d.0)
            } else {
                (d.0, keep.0)
            };
            if !dismissed.query_row(params![a, b], |r| r.get::<_, bool>(0))? {
                out.push(Suggestion {
                    keep: keep.clone(),
                    drop: d,
                });
            }
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.keep.2 + s.drop.2));
    Ok(out)
}

/// «Не предлагать» for a pair.
pub fn dismiss(conn: &Connection, x: ArtistId, y: ArtistId) -> Result<(), LibraryError> {
    let (a, b) = if x < y { (x, y) } else { (y, x) };
    conn.execute(
        "INSERT OR IGNORE INTO artist_pair_dismissed(a, b) VALUES (?1, ?2)",
        params![a, b],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open;
    use crate::repo::{
        Identity, NewTrack, album_get_or_create, artist_get_or_create, track_insert,
    };

    fn lib(tag: &str) -> Connection {
        let root = std::env::temp_dir().join(format!("sirtone-art-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap()
    }

    fn track(c: &mut Connection, n: u32, artists: Vec<ArtistId>, album: Option<i64>) -> TrackId {
        track_insert(
            c,
            &NewTrack {
                rel_path: format!("A/{n:03}.mp3"),
                size_bytes: 5,
                mtime_ms: 0,
                content_hash: format!("h{n}"),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: 1000,
                title: format!("Песня {n}"),
                album_id: album,
                disc_no: None,
                track_no: None,
                year: None,
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists,
            },
        )
        .unwrap()
    }

    fn album_of(c: &Connection, t: TrackId) -> i64 {
        c.query_row("SELECT album_id FROM track WHERE id = ?1", [t], |r| {
            r.get(0)
        })
        .unwrap()
    }

    fn links(c: &Connection, t: TrackId) -> Vec<(ArtistId, String)> {
        let mut st = c
            .prepare("SELECT artist_id, role FROM track_artist WHERE track_id = ?1 ORDER BY role")
            .unwrap();
        st.query_map([t], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn suggest_merge_alias_and_undo() {
        let mut c = lib("merge");
        let cyr = artist_get_or_create(&c, "Би-2").unwrap();
        let lat = artist_get_or_create(&c, "Bi-2").unwrap();
        let beatles = artist_get_or_create(&c, "The Beatles").unwrap();
        let beatles2 = artist_get_or_create(&c, "Beatles").unwrap();
        let other = artist_get_or_create(&c, "Земфира").unwrap();
        let al_k = album_get_or_create(&c, "Мяу кисс ми", cyr, Some(2006)).unwrap();
        let al_d = album_get_or_create(&c, "Myau kiss mi", lat, Some(2006)).unwrap();
        let al_same = album_get_or_create(&c, "Мяу кисс ми", lat, Some(2006)).unwrap();
        let t1 = track(&mut c, 1, vec![cyr], Some(al_k));
        let t2 = track(&mut c, 2, vec![cyr], None);
        let t3 = track(&mut c, 3, vec![lat], Some(al_d));
        let t4 = track(&mut c, 4, vec![lat], Some(al_same));
        // Both spellings on one track (featured + primary).
        let t5 = track(&mut c, 5, vec![lat, cyr], None);
        track(&mut c, 6, vec![beatles], None);
        track(&mut c, 7, vec![beatles2], None);
        track(&mut c, 8, vec![other], None);
        c.execute(
            "INSERT INTO feedback(ts, artist_id, kind) VALUES (1, ?1, 'never')",
            [lat],
        )
        .unwrap();

        let s = suggestions(&c).unwrap();
        assert_eq!(s.len(), 2, "{s:?}");
        let bi = s
            .iter()
            .find(|x| x.keep.0 == cyr)
            .expect("Би-2 kept (more tracks)");
        assert_eq!(bi.drop.0, lat);
        dismiss(&c, beatles2, beatles).unwrap();
        assert_eq!(suggestions(&c).unwrap().len(), 1);

        let before: Vec<_> = [t1, t2, t3, t4, t5].iter().map(|&t| links(&c, t)).collect();
        let rec = merge(&mut c, cyr, lat).unwrap();
        // Links: t3/t4 now Би-2; t5 has Би-2 once, as primary.
        assert_eq!(links(&c, t3), [(cyr, "primary".to_string())]);
        assert_eq!(links(&c, t5), [(cyr, "primary".to_string())]);
        // Albums: the same title joined, the other changed its album artist.

        assert_eq!(album_of(&c, t4), al_k);
        let aa: i64 = c
            .query_row(
                "SELECT album_artist_id FROM album WHERE id = ?1",
                [al_d],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(aa, cyr);
        // ♡/«никогда» follow; the spelling is an alias: a new import lands with Би-2.
        let fb: i64 = c
            .query_row(
                "SELECT count(*) FROM feedback WHERE artist_id = ?1",
                [cyr],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fb, 1);
        assert_eq!(artist_get_or_create(&c, "Bi-2").unwrap(), cyr);
        assert!(suggestions(&c).unwrap().is_empty());

        // Undo restores exactly.
        let back = undo(&mut c, &rec).unwrap();
        assert_eq!(back.len(), rec.tracks.len());
        let after: Vec<_> = [t1, t2, t3, t4, t5].iter().map(|&t| links(&c, t)).collect();
        assert_eq!(after, before);
        assert_eq!(album_of(&c, t4), al_same);
        assert_eq!(artist_get_or_create(&c, "Bi-2").unwrap(), lat);
        assert!(undo(&mut c, &rec).is_err(), "undone once");
        assert!(merge(&mut c, cyr, cyr).is_err());
    }
}
