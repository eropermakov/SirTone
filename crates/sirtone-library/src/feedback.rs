//! Player feedback (T2.19, FR-LEARN-003, FR-WENG-009; `14` §6, §18): a like, «не сейчас» (a
//! minus only in the current context bucket, shelved for 7 days) and «никогда в волне» for a
//! track or its artist. Every action is a fact in `feedback`; «никогда» also sets `wave_never`
//! (the wave's hard mask, Stage 3) and is lifted in Settings. Manual play is never blocked (Q-14).

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::now_ms;
use crate::error::LibraryError;
use crate::repo::{ArtistId, TrackId};

/// «Не сейчас» keeps the track away in this context for a week (`14` §6 penalties).
pub const NOT_NOW_MS: i64 = 7 * 24 * 3600 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrackState {
    pub liked: bool,
    pub never_track: bool,
    pub never_artist: bool,
}

fn insert(
    conn: &Connection,
    track: Option<TrackId>,
    artist: Option<ArtistId>,
    kind: &str,
    ctx_key: Option<&str>,
    expires_at: Option<i64>,
) -> Result<(), LibraryError> {
    conn.execute(
        "INSERT INTO feedback(ts, track_id, artist_id, kind, ctx_key, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![now_ms(), track, artist, kind, ctx_key, expires_at],
    )?;
    Ok(())
}

/// The primary artist of a track.
pub fn primary_artist(conn: &Connection, track: TrackId) -> Result<Option<ArtistId>, LibraryError> {
    Ok(conn
        .query_row(
            "SELECT artist_id FROM track_artist WHERE track_id = ?1 ORDER BY position LIMIT 1",
            [track],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn track_state(conn: &Connection, track: TrackId) -> Result<TrackState, LibraryError> {
    let liked = conn
        .query_row(
            "SELECT kind FROM feedback WHERE track_id = ?1 AND kind IN ('like','unlike')
             ORDER BY id DESC LIMIT 1",
            [track],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .is_some_and(|k| k == "like");
    let (never_track, never_artist): (bool, bool) = conn
        .query_row(
            "SELECT t.wave_never,
                    coalesce((SELECT a.wave_never FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                              WHERE ta.track_id = t.id ORDER BY ta.position LIMIT 1), 0)
             FROM track t WHERE t.id = ?1",
            [track],
            |r| Ok((r.get::<_, i64>(0)? != 0, r.get::<_, i64>(1)? != 0)),
        )
        .optional()?
        .unwrap_or_default();
    Ok(TrackState {
        liked,
        never_track,
        never_artist,
    })
}

pub fn set_like(conn: &Connection, track: TrackId, on: bool) -> Result<(), LibraryError> {
    insert(
        conn,
        Some(track),
        None,
        if on { "like" } else { "unlike" },
        None,
        None,
    )
}

/// «Не сейчас» in the context bucket `ctx_key` (e.g. `daypart:3`).
pub fn not_now(conn: &Connection, track: TrackId, ctx_key: &str) -> Result<(), LibraryError> {
    insert(
        conn,
        Some(track),
        None,
        "not_now",
        Some(ctx_key),
        Some(now_ms() + NOT_NOW_MS),
    )
}

pub fn set_never_track(
    conn: &mut Connection,
    track: TrackId,
    on: bool,
) -> Result<(), LibraryError> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE track SET wave_never = ?2 WHERE id = ?1",
        params![track, i64::from(on)],
    )?;
    insert(
        &tx,
        Some(track),
        None,
        if on { "never" } else { "unnever" },
        None,
        None,
    )?;
    tx.commit()?;
    Ok(())
}

pub fn set_never_artist(
    conn: &mut Connection,
    artist: ArtistId,
    on: bool,
) -> Result<(), LibraryError> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE artist SET wave_never = ?2 WHERE id = ?1",
        params![artist, i64::from(on)],
    )?;
    insert(
        &tx,
        None,
        Some(artist),
        if on { "never" } else { "unnever" },
        None,
        None,
    )?;
    tx.commit()?;
    Ok(())
}

/// Manual track tags (FR-LEARN-013, `14` §17): they override automatic estimates.
pub const TAGS: [&str; 9] = [
    "season_winter",
    "season_spring",
    "season_summer",
    "season_autumn",
    "no_season",
    "sleep",
    "focus",
    "for_all_hit",
    "not_for_company",
];

pub fn track_tags(conn: &Connection, track: TrackId) -> Result<Vec<String>, LibraryError> {
    let mut st =
        conn.prepare("SELECT tag FROM track_tag WHERE track_id = ?1 AND tag <> 'event'")?;
    let rows = st
        .query_map([track], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Sets or clears a tag. «no_season» and the season tags exclude each other.
pub fn set_tag(
    conn: &mut Connection,
    track: TrackId,
    tag: &str,
    on: bool,
) -> Result<(), LibraryError> {
    if !TAGS.contains(&tag) {
        return Err(LibraryError::Invalid(format!("unknown tag {tag}")));
    }
    let tx = conn.transaction()?;
    if on {
        if tag == "no_season" {
            tx.execute(
                "DELETE FROM track_tag WHERE track_id = ?1 AND tag LIKE 'season_%'",
                [track],
            )?;
        } else if tag.starts_with("season_") {
            tx.execute(
                "DELETE FROM track_tag WHERE track_id = ?1 AND tag = 'no_season'",
                [track],
            )?;
        }
        tx.execute(
            "INSERT OR IGNORE INTO track_tag(track_id, tag, value, created_at) VALUES (?1, ?2, '', ?3)",
            params![track, tag, now_ms()],
        )?;
    } else {
        tx.execute(
            "DELETE FROM track_tag WHERE track_id = ?1 AND tag = ?2",
            params![track, tag],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// A row of «Никогда в волне» in Settings.
#[derive(Debug, Clone, PartialEq)]
pub enum NeverItem {
    Track {
        id: TrackId,
        title: String,
        artists: String,
    },
    Artist {
        id: ArtistId,
        name: String,
    },
}

pub fn never_list(conn: &Connection) -> Result<Vec<NeverItem>, LibraryError> {
    let mut out = Vec::new();
    let mut st = conn
        .prepare("SELECT a.id, a.name FROM artist a WHERE a.wave_never = 1 ORDER BY a.name_norm")?;
    for r in st.query_map([], |r| {
        Ok(NeverItem::Artist {
            id: r.get(0)?,
            name: r.get(1)?,
        })
    })? {
        out.push(r?);
    }
    let mut st = conn.prepare(
        "SELECT t.id, t.title,
                coalesce((SELECT group_concat(name, ', ') FROM
                  (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                   WHERE ta.track_id = t.id ORDER BY ta.position)), '')
         FROM track t WHERE t.wave_never = 1 ORDER BY t.title_norm",
    )?;
    for r in st.query_map([], |r| {
        Ok(NeverItem::Track {
            id: r.get(0)?,
            title: r.get(1)?,
            artists: r.get(2)?,
        })
    })? {
        out.push(r?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open;
    use crate::repo::{Identity, NewTrack, artist_get_or_create, track_insert};

    #[test]
    fn like_not_now_and_never_round_trip() {
        let root = std::env::temp_dir().join(format!("sirtone-fb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut c = open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap();
        let kino = artist_get_or_create(&c, "Кино").unwrap();
        let t = track_insert(
            &mut c,
            &NewTrack {
                rel_path: "Кино/x.mp3".into(),
                size_bytes: 5,
                mtime_ms: 0,
                content_hash: "h".into(),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: 1000,
                title: "Кукушка".into(),
                album_id: None,
                disc_no: None,
                track_no: None,
                year: None,
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![kino],
            },
        )
        .unwrap();
        assert_eq!(track_state(&c, t).unwrap(), TrackState::default());
        set_like(&c, t, true).unwrap();
        assert!(track_state(&c, t).unwrap().liked);
        set_like(&c, t, false).unwrap();
        assert!(
            !track_state(&c, t).unwrap().liked,
            "the latest like/unlike wins"
        );
        not_now(&c, t, "daypart:3").unwrap();
        let (ctx, exp): (String, i64) = c
            .query_row(
                "SELECT ctx_key, expires_at FROM feedback WHERE kind = 'not_now'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(ctx == "daypart:3" && exp > now_ms() + NOT_NOW_MS - 60_000);
        set_never_track(&mut c, t, true).unwrap();
        let artist = primary_artist(&c, t).unwrap().unwrap();
        set_never_artist(&mut c, artist, true).unwrap();
        let s = track_state(&c, t).unwrap();
        assert!(s.never_track && s.never_artist);
        assert_eq!(never_list(&c).unwrap().len(), 2);
        set_never_track(&mut c, t, false).unwrap();
        set_never_artist(&mut c, kino, false).unwrap();
        assert!(never_list(&c).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tags_toggle_and_seasons_exclude_no_season() {
        let root = std::env::temp_dir().join(format!("sirtone-tags-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut c = open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap();
        let t = track_insert(
            &mut c,
            &NewTrack {
                rel_path: "x.mp3".into(),
                size_bytes: 5,
                mtime_ms: 0,
                content_hash: "h".into(),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: 1000,
                title: "X".into(),
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
        .unwrap();
        set_tag(&mut c, t, "season_summer", true).unwrap();
        set_tag(&mut c, t, "sleep", true).unwrap();
        assert_eq!(track_tags(&c, t).unwrap().len(), 2);
        set_tag(&mut c, t, "no_season", true).unwrap();
        let mut tags = track_tags(&c, t).unwrap();
        tags.sort();
        assert_eq!(tags, ["no_season", "sleep"]);
        set_tag(&mut c, t, "sleep", false).unwrap();
        assert_eq!(track_tags(&c, t).unwrap(), ["no_season"]);
        assert!(set_tag(&mut c, t, "bogus", true).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
