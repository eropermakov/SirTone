//! Loudness of library tracks for normalization (T4.11, `04` §5): the own measurement (B0,
//! EBU R128, true peak) wins over ReplayGain tags; an album's loudness is computed here from its
//! tracks (duration-weighted power average, loudest peak) and only when every track of it is
//! known — `album.lufs_album` is not kept (it could not go stale this way, D-97).

use std::collections::HashMap;

use rusqlite::{Connection, params_from_iter};
use sirtone_core::loudness::{Loudness, album};

use crate::error::LibraryError;

/// A track's loudness and its album's (when the whole album is known).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrackLoudness {
    pub track: Loudness,
    pub album: Option<Loudness>,
}

struct Row {
    album: Option<i64>,
    measured: Loudness,
    tagged: Loudness,
    tagged_album: Loudness,
    duration_ms: u64,
}

fn pick(r: &Row) -> Loudness {
    if r.measured.lufs.is_some() {
        r.measured
    } else {
        r.tagged
    }
}

pub fn for_tracks(
    conn: &Connection,
    ids: &[i64],
) -> Result<HashMap<i64, TrackLoudness>, LibraryError> {
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(i64, Row)> {
        Ok((
            r.get(0)?,
            Row {
                album: r.get(1)?,
                measured: Loudness {
                    lufs: r.get::<_, Option<f64>>(2)?.map(|v| v as f32),
                    peak_db: r.get::<_, Option<f64>>(3)?.map(|v| v as f32),
                },
                tagged: Loudness {
                    lufs: r.get::<_, Option<f64>>(4)?.map(|v| v as f32),
                    peak_db: r.get::<_, Option<f64>>(5)?.map(|v| v as f32),
                },
                tagged_album: Loudness {
                    lufs: r.get::<_, Option<f64>>(6)?.map(|v| v as f32),
                    peak_db: r.get::<_, Option<f64>>(7)?.map(|v| v as f32),
                },
                duration_ms: r.get::<_, i64>(8)?.max(0) as u64,
            },
        ))
    };
    const COLS: &str = "id, album_id, lufs_i, true_peak_db, tag_lufs, tag_peak_db, tag_lufs_album,
                        tag_peak_album_db, duration_ms";
    let mut tracks = HashMap::new();
    for chunk in ids.chunks(500) {
        let marks = vec!["?"; chunk.len()].join(",");
        let mut st = conn.prepare(&format!("SELECT {COLS} FROM track WHERE id IN ({marks})"))?;
        for r in st.query_map(params_from_iter(chunk), row)? {
            let (id, r) = r?;
            tracks.insert(id, r);
        }
    }
    // Albums: every active track of each album involved.
    let mut albums: HashMap<i64, Option<Loudness>> = HashMap::new();
    let mut st = conn.prepare_cached(&format!(
        "SELECT {COLS} FROM track WHERE album_id = ?1 AND state = 'ok'"
    ))?;
    for r in tracks.values() {
        let Some(a) = r.album else { continue };
        if albums.contains_key(&a) {
            continue;
        }
        let members: Vec<Row> = st
            .query_map([a], row)?
            .map(|x| x.map(|(_, r)| r))
            .collect::<Result<_, _>>()?;
        let measured: Vec<(Loudness, u64)> =
            members.iter().map(|m| (pick(m), m.duration_ms)).collect();
        // The whole album known (measured or tagged per track) → computed; else the tagger's
        // album value when every track carries it; else none (track gain meanwhile).
        let value = album(&measured).or_else(|| {
            let first = members.first()?.tagged_album;
            (first.lufs.is_some() && members.iter().all(|m| m.tagged_album == first))
                .then_some(first)
        });
        albums.insert(a, value);
    }
    for (id, r) in &tracks {
        out.insert(
            *id,
            TrackLoudness {
                track: pick(r),
                album: r.album.and_then(|a| albums.get(&a).copied().flatten()),
            },
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measurement_wins_and_albums_need_every_track() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO artist(id, uid, name, name_norm, created_at) VALUES (1, 'a1', 'A', 'a', 0);
             INSERT INTO album(id, uid, title, title_norm, album_artist_id, created_at) VALUES (1, 'u1', 'X', 'x', 1, 0);",
        )
        .unwrap();
        let add = |id: i64, lufs: Option<f64>, peak: Option<f64>, tag: Option<f64>| {
            conn.execute(
                "INSERT INTO track(id, uid, rel_path, size_bytes, mtime_ms, content_hash, container,
                   codec, lossless, duration_ms, title, title_norm, album_id, identity_state,
                   lufs_i, true_peak_db, tag_lufs, tag_peak_db, added_at)
                 VALUES (?1, ?1, ?1, 1, 1, ?1, 'flac', 'flac', 1, 180000, 't', 't', 1,
                   'confirmed', ?2, ?3, ?4, -3.0, 0)",
                rusqlite::params![id, lufs, peak, tag],
            )
            .unwrap();
        };
        add(1, Some(-9.0), Some(-0.3), Some(-20.0));
        add(2, None, None, Some(-12.0));
        let l = for_tracks(&conn, &[1, 2]).unwrap();
        // Measured beats the tag.
        assert_eq!(l[&1].track.lufs, Some(-9.0));
        assert_eq!(l[&1].track.peak_db, Some(-0.3));
        // Tag when not measured.
        assert_eq!(l[&2].track.lufs, Some(-12.0));
        assert_eq!(l[&2].track.peak_db, Some(-3.0));
        // Both known → one album value for both.
        assert!(l[&1].album.is_some());
        assert_eq!(l[&1].album, l[&2].album);
        // A third, unknown track: no album value any more.
        add(3, None, None, None);
        let l = for_tracks(&conn, &[1]).unwrap();
        assert_eq!(l[&1].album, None);
    }
}
