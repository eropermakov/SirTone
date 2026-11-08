//! What the transfer package (PC ↔ phone, D-140, M6.2) reads from a library. Plain rows only:
//! the composition root maps them onto `sirtone-transfer` (domain crates do not depend on each
//! other).

use rusqlite::{Connection, OptionalExtension};

use crate::LibraryError;

/// The library's own id (`meta.library_uid`): prefix of every key in a package.
pub fn library_uid(conn: &Connection) -> Result<String, LibraryError> {
    Ok(conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'library_uid'",
            [],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_default())
}

/// A track as the package needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct TransferTrack {
    pub id: i64,
    pub rel_path: String,
    pub size: u64,
    pub duration_ms: u32,
    pub artist: Option<String>,
    pub title: String,
    pub album: Option<String>,
    /// Compressed Chromaprint as stored (`sirtone_core::fingerprint::decompress`).
    pub fingerprint: Option<Vec<u8>>,
    /// BLAKE3 of the whole file (computed on import): the same file on both sides.
    pub content_hash: String,
    /// The file is an economy copy at this level (D-146).
    pub copy: Option<String>,
}

/// Every track of the library, in id order.
pub fn tracks(conn: &Connection) -> Result<Vec<TransferTrack>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT t.id, t.rel_path, t.size_bytes, t.duration_ms, t.title, al.title,
                (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                  WHERE ta.track_id = t.id AND ta.role = 'primary' ORDER BY ta.position LIMIT 1),
                t.fingerprint, t.content_hash,
                CASE WHEN t.is_copy = 1 THEN t.copy_level END
           FROM track t LEFT JOIN album al ON al.id = t.album_id
          ORDER BY t.id",
    )?;
    let rows = st.query_map([], |r| {
        Ok(TransferTrack {
            id: r.get(0)?,
            rel_path: r.get(1)?,
            size: r.get::<_, i64>(2)?.max(0) as u64,
            duration_ms: r.get::<_, i64>(3)?.clamp(0, i64::from(u32::MAX)) as u32,
            title: r.get(4)?,
            album: r.get(5)?,
            artist: r.get(6)?,
            fingerprint: r.get(7)?,
            content_hash: r.get(8)?,
            copy: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The context columns of a `play_event` row, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayContext {
    pub local_minute: u16,
    pub weekday: u8,
    pub daypart: u8,
    /// `u8[4]`, ×255.
    pub season_w: [u8; 4],
    pub weather: Option<u8>,
}

/// One journal line for the package: a play (`play_event`) or a track mark (`feedback`).
#[derive(Debug, Clone, PartialEq)]
pub enum JournalRow {
    Play {
        id: i64,
        ts: i64,
        track_id: i64,
        played_ms: i64,
        duration_ms: i64,
        end_reason: String,
        source: String,
        /// Context of the play as the source recorded it (the wave learns per context).
        context: PlayContext,
    },
    Mark {
        id: i64,
        ts: i64,
        track_id: i64,
        kind: String,
    },
}

/// The whole journal: plays and track marks (artist marks stay on the device), oldest first.
pub fn journal(conn: &Connection) -> Result<Vec<JournalRow>, LibraryError> {
    let mut out = Vec::new();
    let mut st = conn.prepare(
        "SELECT id, ts, track_id, played_ms, duration_ms, end_reason, source,
                local_minute, weekday, daypart, season_w, weather
           FROM play_event ORDER BY ts, id",
    )?;
    for row in st.query_map([], |r| {
        Ok(JournalRow::Play {
            id: r.get(0)?,
            ts: r.get(1)?,
            track_id: r.get(2)?,
            played_ms: r.get(3)?,
            duration_ms: r.get(4)?,
            end_reason: r.get(5)?,
            source: r.get(6)?,
            context: {
                let w: Vec<u8> = r.get(10)?;
                PlayContext {
                    local_minute: r.get(7)?,
                    weekday: r.get(8)?,
                    daypart: r.get(9)?,
                    season_w: [0, 1, 2, 3].map(|i| w.get(i).copied().unwrap_or(0)),
                    weather: r.get(11)?,
                }
            },
        })
    })? {
        out.push(row?);
    }
    let mut st = conn.prepare(
        "SELECT id, ts, track_id, kind FROM feedback
          WHERE track_id IS NOT NULL ORDER BY ts, id",
    )?;
    for row in st.query_map([], |r| {
        Ok(JournalRow::Mark {
            id: r.get(0)?,
            ts: r.get(1)?,
            track_id: r.get(2)?,
            kind: r.get(3)?,
        })
    })? {
        out.push(row?);
    }
    Ok(out)
}

/// Keys already applied from packages (and so never applied again).
pub fn seen_keys(conn: &Connection) -> Result<std::collections::HashSet<String>, LibraryError> {
    let mut st = conn.prepare("SELECT key FROM transfer_seen")?;
    let keys = st.query_map([], |r| r.get(0))?;
    Ok(keys.collect::<Result<_, _>>()?)
}

fn mark_seen(tx: &Connection, key: &str) -> Result<bool, LibraryError> {
    Ok(tx.execute(
        "INSERT INTO transfer_seen(key, applied_at) VALUES (?1, ?2) ON CONFLICT(key) DO NOTHING",
        rusqlite::params![key, crate::db::now_ms()],
    )? == 1)
}

/// A play from a package: into the journal and the track statistics (the same path as a play
/// heard here), once per key. Returns false when the key was applied before.
#[allow(clippy::too_many_arguments)]
pub fn apply_play(
    conn: &Connection,
    key: &str,
    track_id: i64,
    ts: i64,
    played_ms: u64,
    duration_ms: u64,
    end_reason: &str,
    source: &str,
    context: &PlayContext,
) -> Result<bool, LibraryError> {
    use sirtone_core::wave::{ContextSnapshot, PlayEvent};
    let tx = conn.unchecked_transaction()?;
    if !mark_seen(&tx, key)? {
        return Ok(false);
    }
    // The same play coming back under the other device's key (it was sent there before and
    // is in its journal now): one play, one row.
    if echo(&tx, "play_event", track_id, ts, None)? {
        tx.commit()?;
        return Ok(false);
    }
    let e = PlayEvent {
        ts,
        track_id,
        played_ms,
        duration_ms,
        end_reason: crate::wave::end_reason(end_reason),
        source: crate::wave::source(source),
        profile: None,
        context: ContextSnapshot {
            now_utc_ms: ts,
            local_minute: context.local_minute,
            weekday: context.weekday,
            daypart: crate::wave::daypart(context.daypart),
            season_w: context.season_w.map(|b| f32::from(b) / 255.0),
            weather: context.weather.map(crate::wave::weather),
        },
        event_id: None,
        novelty: None,
        category: None,
    };
    // As `wave::record_play`, inside this transaction.
    crate::wave::insert_event(&tx, &e)?;
    crate::stats::apply(&tx, &e)?;
    tx.commit()?;
    Ok(true)
}

/// A mark from a package, once per key, keeping its original time. `like`/`unlike` are written
/// only when the merge decided the state changes (the latest mark wins, D-140); `never`/`unnever`
/// switch the wave exclusion; `not_now` belongs to the device's own moment and is skipped.
pub fn apply_mark(
    conn: &Connection,
    key: &str,
    track_id: i64,
    ts: i64,
    kind: &str,
    write_like: bool,
) -> Result<bool, LibraryError> {
    let tx = conn.unchecked_transaction()?;
    if !mark_seen(&tx, key)? {
        return Ok(false);
    }
    if echo(&tx, "feedback", track_id, ts, Some(kind))? {
        tx.commit()?;
        return Ok(false);
    }
    let write = match kind {
        "like" | "unlike" => write_like,
        "never" | "unnever" => {
            tx.execute(
                "UPDATE track SET wave_never = ?2 WHERE id = ?1",
                rusqlite::params![track_id, i64::from(kind == "never")],
            )?;
            true
        }
        _ => false,
    };
    if write {
        tx.execute(
            "INSERT INTO feedback(ts, track_id, kind) VALUES (?1, ?2, ?3)",
            rusqlite::params![ts, track_id, kind],
        )?;
    }
    tx.commit()?;
    Ok(true)
}

/// A row for this track at exactly this millisecond is already here: the event has travelled
/// out and back (a package carries every journal row, also those that came from packages).
fn echo(
    tx: &Connection,
    table: &str,
    track_id: i64,
    ts: i64,
    kind: Option<&str>,
) -> Result<bool, LibraryError> {
    let sql = match (table, kind) {
        ("play_event", _) => {
            "SELECT EXISTS(SELECT 1 FROM play_event WHERE track_id = ?1 AND ts = ?2)"
        }
        (_, Some(_)) => {
            "SELECT EXISTS(SELECT 1 FROM feedback WHERE track_id = ?1 AND ts = ?2 AND kind = ?3)"
        }
        _ => return Ok(false),
    };
    let found: bool = match kind {
        Some(k) => tx.query_row(sql, rusqlite::params![track_id, ts, k], |r| r.get(0))?,
        None => tx.query_row(sql, rusqlite::params![track_id, ts], |r| r.get(0))?,
    };
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::repo::{
        Identity, NewTrack, album_get_or_create, artist_get_or_create, track_insert,
    };

    #[test]
    fn tracks_and_journal_for_a_package() {
        let mut c = open_in_memory().unwrap();
        let kino = artist_get_or_create(&c, "Кино").unwrap();
        let album = album_get_or_create(&c, "Группа крови", kino, Some(1988)).unwrap();
        let id = track_insert(
            &mut c,
            &NewTrack {
                rel_path: "Кино/Группа крови/01 Группа крови.flac".into(),
                size_bytes: 30_000_000,
                mtime_ms: 0,
                content_hash: "h".into(),
                container: "flac".into(),
                codec: "flac".into(),
                lossless: true,
                sample_rate: Some(44100),
                channels: Some(2),
                duration_ms: 287_000,
                title: "Группа крови".into(),
                album_id: Some(album),
                disc_no: None,
                track_no: Some(1),
                year: Some(1988),
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![kino],
            },
        )
        .unwrap();
        assert!(!library_uid(&c).unwrap().is_empty());
        let t = tracks(&c).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].artist.as_deref(), Some("Кино"));
        assert_eq!(t[0].album.as_deref(), Some("Группа крови"));
        assert_eq!((t[0].size, t[0].duration_ms), (30_000_000, 287_000));

        crate::feedback::set_like(&c, id, true).unwrap();
        let j = journal(&c).unwrap();
        assert!(
            j.iter()
                .any(|r| matches!(r, JournalRow::Mark { kind, track_id, .. } if kind == "like" && *track_id == id)),
            "{j:?}"
        );

        // A play applied twice lands once and counts in the statistics once.
        let ctx = PlayContext {
            local_minute: 600,
            weekday: 2,
            daypart: 1,
            season_w: [0, 0, 0, 255],
            weather: None,
        };
        assert!(
            apply_play(
                &c,
                "phone:play:1",
                id,
                1_000,
                200_000,
                287_000,
                "finished",
                "wave",
                &ctx
            )
            .unwrap()
        );
        assert!(
            !apply_play(
                &c,
                "phone:play:1",
                id,
                1_000,
                200_000,
                287_000,
                "finished",
                "wave",
                &ctx
            )
            .unwrap()
        );
        let plays: i64 = c
            .query_row(
                "SELECT count(*) FROM play_event WHERE track_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(plays, 1);
        let j = journal(&c).unwrap();
        assert!(
            j.iter()
                .any(|r| matches!(r, JournalRow::Play { context, .. } if *context == ctx))
        );
        assert!(seen_keys(&c).unwrap().contains("phone:play:1"));

        // «Не нравится» from the phone wins when the merge says so; «никогда» switches the wave off.
        assert!(apply_mark(&c, "phone:mark:1", id, 2_000, "unlike", true).unwrap());
        assert!(!crate::feedback::track_state(&c, id).unwrap().liked);
        assert!(apply_mark(&c, "phone:mark:2", id, 3_000, "never", false).unwrap());
        assert!(crate::feedback::track_state(&c, id).unwrap().never_track);
        assert!(!apply_mark(&c, "phone:mark:2", id, 3_000, "never", false).unwrap());

        // Out and back: the same play and mark under the other device's own keys land nowhere.
        assert!(
            !apply_play(
                &c,
                "pc:play:77",
                id,
                1_000,
                200_000,
                287_000,
                "finished",
                "wave",
                &ctx
            )
            .unwrap()
        );
        assert!(!apply_mark(&c, "pc:mark:9", id, 3_000, "never", false).unwrap());
        let plays: i64 = c
            .query_row(
                "SELECT count(*) FROM play_event WHERE track_id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(plays, 1, "an echo is not a second play");
        let marks: i64 = c
            .query_row(
                "SELECT count(*) FROM feedback WHERE track_id = ?1 AND kind = 'never'",
                [id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(marks, 1);
    }
}

/// The analysis columns a package carries (D-148): a white list, as every dynamic SQL here.
/// Fingerprints travel separately (`TrackEntry::fingerprint`); play statistics never travel.
const ANALYSIS: [&str; 28] = [
    "lufs_i",
    "true_peak_db",
    "lra",
    "peaks",
    "lead_silence_ms",
    "tail_silence_ms",
    "env_head",
    "env_tail",
    "bpm",
    "bpm_conf",
    "beat_grid",
    "grid_stability",
    "key_camelot",
    "key_conf",
    "valence",
    "arousal",
    "aggression",
    "energy",
    "danceability",
    "acousticness",
    "vocal",
    "lang",
    "lang_conf",
    "mood_bits",
    "activity_json",
    "embedding",
    "analysis_version",
    "analyzed_at",
];
/// The wave's feature row (`track_features`), under these keys in the same map.
const FEATURES: [&str; 5] = ["version", "vec", "conf", "season_aff", "kw_bits"];

fn to_json(v: rusqlite::types::Value) -> serde_json::Value {
    use rusqlite::types::Value as V;
    match v {
        V::Null => serde_json::Value::Null,
        V::Integer(i) => i.into(),
        V::Real(f) => serde_json::Number::from_f64(f).map_or(serde_json::Value::Null, Into::into),
        V::Text(s) => s.into(),
        V::Blob(b) => serde_json::json!({ "b": sirtone_core::fingerprint::base64url(&b) }),
    }
}

fn from_json(v: &serde_json::Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as V;
    match v {
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(V::Integer)
            .or_else(|| n.as_f64().map(V::Real))
            .unwrap_or(V::Null),
        serde_json::Value::String(s) => V::Text(s.clone()),
        serde_json::Value::Object(o) => o
            .get("b")
            .and_then(serde_json::Value::as_str)
            .and_then(sirtone_core::fingerprint::unbase64url)
            .map_or(V::Null, V::Blob),
        _ => V::Null,
    }
}

/// A track's analysis for the package; `None` — not analysed.
pub fn analysis_of(
    conn: &Connection,
    track_id: i64,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, LibraryError> {
    let sql = format!(
        "SELECT {} FROM track WHERE id = ?1 AND analysis_version > 0",
        ANALYSIS.join(", ")
    );
    let Some(mut m) = conn
        .query_row(&sql, [track_id], |r| {
            let mut m = serde_json::Map::new();
            for (i, c) in ANALYSIS.iter().enumerate() {
                let v = to_json(r.get(i)?);
                if !v.is_null() {
                    m.insert((*c).to_owned(), v);
                }
            }
            Ok(m)
        })
        .optional()?
    else {
        return Ok(None);
    };
    let f = conn
        .query_row(
            &format!(
                "SELECT {} FROM track_features WHERE track_id = ?1",
                FEATURES.join(", ")
            ),
            [track_id],
            |r| {
                let mut f = serde_json::Map::new();
                for (i, c) in FEATURES.iter().enumerate() {
                    let v = to_json(r.get(i)?);
                    if !v.is_null() {
                        f.insert((*c).to_owned(), v);
                    }
                }
                Ok(f)
            },
        )
        .optional()?;
    if let Some(f) = f {
        m.insert("features".into(), f.into());
    }
    Ok(Some(m))
}

/// Writes a package's analysis over this library's track when it is newer (a higher
/// `analysis_version`) or the track has none. Returns whether it was written.
pub fn apply_analysis(
    conn: &Connection,
    track_id: i64,
    a: &serde_json::Map<String, serde_json::Value>,
) -> Result<bool, LibraryError> {
    let theirs = a
        .get("analysis_version")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let ours: i64 = conn
        .query_row(
            "SELECT analysis_version FROM track WHERE id = ?1",
            [track_id],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(i64::MAX);
    if theirs <= ours {
        return Ok(false);
    }
    let cols: Vec<&str> = ANALYSIS
        .iter()
        .copied()
        .filter(|c| a.contains_key(*c))
        .collect();
    let set = cols
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{c} = ?{}", i + 2))
        .collect::<Vec<_>>()
        .join(", ");
    let mut vals: Vec<rusqlite::types::Value> = vec![track_id.into()];
    vals.extend(cols.iter().map(|c| from_json(&a[*c])));
    conn.execute(
        &format!("UPDATE track SET {set} WHERE id = ?1"),
        rusqlite::params_from_iter(vals),
    )?;
    if let Some(f) = a.get("features").and_then(serde_json::Value::as_object)
        && f.contains_key("vec")
    {
        let g = |k: &str| f.get(k).map_or(rusqlite::types::Value::Null, from_json);
        conn.execute(
            "INSERT INTO track_features (track_id, version, vec, conf, season_aff, kw_bits, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, coalesce(?6, 0), strftime('%s','now') * 1000)
             ON CONFLICT(track_id) DO UPDATE SET version = excluded.version, vec = excluded.vec,
               conf = excluded.conf, season_aff = excluded.season_aff, kw_bits = excluded.kw_bits,
               updated_at = excluded.updated_at",
            rusqlite::params![
                track_id,
                g("version"),
                g("vec"),
                g("conf"),
                g("season_aff"),
                g("kw_bits")
            ],
        )?;
    }
    Ok(true)
}
