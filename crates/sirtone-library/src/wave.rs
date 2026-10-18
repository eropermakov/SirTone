//! Wave data in `library.db` (docs/14 §18, T1.30): the listening journal. The repository keeps
//! its own connection (WAL lets it write next to the import worker); each event is one
//! transaction, so a crash loses at most the event in flight (T1.31).

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::wave::{
    ContextSnapshot, Daypart, EndReason, FEATURES, FeedbackRow, NotNowRow, PlayEvent, PlaySource,
    WaveRepo, WaveTrack, WeatherClass, season_w_bytes,
};

use crate::db::{self, Medium};
use crate::error::LibraryError;

/// The journal row and the track aggregates (T3.17) in one transaction.
pub fn record_play(conn: &Connection, e: &PlayEvent) -> Result<(), LibraryError> {
    let tx = conn.unchecked_transaction()?;
    insert_event(&tx, e)?;
    crate::stats::apply(&tx, e)?;
    tx.commit()?;
    Ok(())
}

fn insert_event(conn: &Connection, e: &PlayEvent) -> Result<(), LibraryError> {
    let c = &e.context;
    conn.execute(
        "INSERT INTO play_event(ts, track_id, played_ms, duration_ms, end_reason, source, profile,
                                local_minute, weekday, daypart, season_w, weather, event_id,
                                novelty, category)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            e.ts,
            e.track_id,
            e.played_ms as i64,
            e.duration_ms as i64,
            e.end_reason.as_str(),
            e.source.as_str(),
            e.profile,
            c.local_minute,
            c.weekday,
            c.daypart as u8,
            season_w_bytes(&c.season_w).as_slice(),
            c.weather.map(|w| w as u8),
            e.event_id,
            e.novelty,
            e.category,
        ],
    )?;
    Ok(())
}

fn end_reason(s: &str) -> EndReason {
    match s {
        "finished" => EndReason::Finished,
        "mixed_out" => EndReason::MixedOut,
        "skip" => EndReason::Skip,
        "sleep" => EndReason::Sleep,
        "stop" => EndReason::Stop,
        "quit" => EndReason::Quit,
        _ => EndReason::Error,
    }
}

fn source(s: &str) -> PlaySource {
    match s {
        "wave" => PlaySource::Wave,
        "search" => PlaySource::Search,
        "collection" => PlaySource::Collection,
        "playlist" => PlaySource::Playlist,
        "album" => PlaySource::Album,
        "artist" => PlaySource::Artist,
        "queue" => PlaySource::Queue,
        "hybrid" => PlaySource::Hybrid,
        _ => PlaySource::External,
    }
}

fn daypart(v: u8) -> Daypart {
    match v {
        0 => Daypart::Dawn,
        1 => Daypart::Morning,
        2 => Daypart::Day,
        3 => Daypart::Evening,
        4 => Daypart::Dusk,
        _ => Daypart::Night,
    }
}

fn weather(v: u8) -> WeatherClass {
    match v {
        0 => WeatherClass::Clear,
        1 => WeatherClass::Cloudy,
        2 => WeatherClass::Fog,
        3 => WeatherClass::DrizzleRain,
        4 => WeatherClass::Snow,
        _ => WeatherClass::Storm,
    }
}

pub fn plays_since(conn: &Connection, since_ms: i64) -> Result<Vec<PlayEvent>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT ts, track_id, played_ms, duration_ms, end_reason, source, profile, local_minute,
                weekday, daypart, season_w, weather, event_id, novelty, category
         FROM play_event WHERE ts >= ?1 ORDER BY ts, id",
    )?;
    let rows = st.query_map([since_ms], |r| {
        let w: Vec<u8> = r.get(10)?;
        let mut season_w = [0f32; 4];
        for (i, b) in w.iter().take(4).enumerate() {
            season_w[i] = f32::from(*b) / 255.0;
        }
        let ts: i64 = r.get(0)?;
        Ok(PlayEvent {
            ts,
            track_id: r.get(1)?,
            played_ms: r.get::<_, i64>(2)?.max(0) as u64,
            duration_ms: r.get::<_, i64>(3)?.max(0) as u64,
            end_reason: end_reason(&r.get::<_, String>(4)?),
            source: source(&r.get::<_, String>(5)?),
            profile: r.get(6)?,
            context: ContextSnapshot {
                now_utc_ms: ts,
                local_minute: r.get(7)?,
                weekday: r.get(8)?,
                daypart: daypart(r.get(9)?),
                season_w,
                weather: r.get::<_, Option<u8>>(11)?.map(weather),
            },
            event_id: r.get(12)?,
            novelty: r.get(13)?,
            category: r.get(14)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// `WaveRepo` over its own connection to `library.db`.
pub struct SqliteWaveRepo {
    conn: Mutex<Connection>,
}

impl SqliteWaveRepo {
    pub fn open(path: &Path, medium: Medium) -> Result<Self, LibraryError> {
        Ok(Self {
            conn: Mutex::new(db::open_existing(path, medium)?),
        })
    }

    pub fn from_connection(conn: Connection) -> Self {
        Self {
            conn: Mutex::new(conn),
        }
    }
}

fn app_err(e: LibraryError) -> AppError {
    AppError::new(ErrorCode::Internal).with_details(e)
}

/// Every playable track for the wave engine (docs/14 §4, §8): statistics, the latest
/// like/unlike, «никогда» of the track or its primary artist, analysed features if any.
pub fn catalog(conn: &Connection) -> Result<Vec<WaveTrack>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT t.id,
                coalesce((SELECT ta.artist_id FROM track_artist ta WHERE ta.track_id = t.id
                          ORDER BY ta.position LIMIT 1), -t.id),
                t.duration_ms, coalesce(t.year, al.year), t.genre_main, t.added_at,
                t.play_count, t.skip_count, t.last_played_at, t.completion_ema,
                coalesce((SELECT f.kind = 'like' FROM feedback f WHERE f.track_id = t.id
                          AND f.kind IN ('like','unlike') ORDER BY f.id DESC LIMIT 1), 0),
                t.wave_never OR coalesce((SELECT a.wave_never FROM track_artist ta
                          JOIN artist a ON a.id = ta.artist_id WHERE ta.track_id = t.id
                          ORDER BY ta.position LIMIT 1), 0),
                tf.vec, tf.conf, t.lufs_i, t.lra, t.env_head, t.env_tail,
                t.title, al.title,
                (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                 WHERE ta.track_id = t.id ORDER BY ta.position LIMIT 1),
                coalesce((SELECT sum(CASE tg.tag WHEN 'season_winter' THEN 1 WHEN 'season_spring' THEN 2
                          WHEN 'season_summer' THEN 4 WHEN 'season_autumn' THEN 8
                          WHEN 'no_season' THEN 16 ELSE 0 END)
                          FROM track_tag tg WHERE tg.track_id = t.id), 0)
         FROM track t LEFT JOIN album al ON al.id = t.album_id
         LEFT JOIN track_features tf ON tf.track_id = t.id
         WHERE t.state = 'ok'",
    )?;
    let rows = st
        .query_map([], |r| {
            let vec: Option<Vec<u8>> = r.get(12)?;
            let conf: Option<f64> = r.get(13)?;
            let features = match (vec, conf) {
                (Some(v), Some(c)) if v.len() == FEATURES * 4 => {
                    let mut x = [0f32; FEATURES];
                    for (i, chunk) in v.as_chunks::<4>().0.iter().enumerate() {
                        x[i] = f32::from_le_bytes(*chunk);
                    }
                    Some((x, c as f32))
                }
                _ => None,
            };
            Ok(WaveTrack {
                id: r.get(0)?,
                artist_id: r.get(1)?,
                duration_ms: r.get::<_, i64>(2)?.max(0) as u64,
                year: r.get(3)?,
                genre: r.get(4)?,
                added_ms: r.get(5)?,
                play_count: r.get(6)?,
                skip_count: r.get(7)?,
                last_played_ms: r.get(8)?,
                completion: r.get::<_, f64>(9)? as f32,
                liked: r.get::<_, i64>(10)? != 0,
                never: r.get::<_, i64>(11)? != 0,
                features,
                lufs: r.get::<_, Option<f64>>(14)?.map(|v| v as f32),
                lra: r.get::<_, Option<f64>>(15)?.map(|v| v as f32),
                soft_intro: r
                    .get::<_, Option<Vec<u8>>>(16)?
                    .and_then(|e| sirtone_core::wave::soft_intro(&e)),
                soft_outro: r
                    .get::<_, Option<Vec<u8>>>(17)?
                    .and_then(|e| sirtone_core::wave::soft_outro(&e)),
                title: r.get(18)?,
                album: r.get(19)?,
                artist_name: r.get(20)?,
                season_tags: r.get::<_, i64>(21)?.clamp(0, 31) as u8,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

pub fn feedback_since(conn: &Connection, since_ms: i64) -> Result<Vec<FeedbackRow>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT ts, track_id, artist_id, kind, ctx_key FROM feedback WHERE ts >= ?1 ORDER BY id",
    )?;
    let rows = st
        .query_map([since_ms], |r| {
            Ok(FeedbackRow {
                ts: r.get(0)?,
                track_id: r.get(1)?,
                artist_id: r.get(2)?,
                kind: r.get(3)?,
                ctx_key: r.get(4)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

pub fn state(conn: &Connection, key: &str) -> Result<Option<String>, LibraryError> {
    Ok(conn
        .query_row("SELECT value FROM wave_state WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set_state(conn: &Connection, key: &str, value: Option<&str>) -> Result<(), LibraryError> {
    match value {
        Some(v) => conn.execute(
            "INSERT INTO wave_state(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, v],
        )?,
        None => conn.execute("DELETE FROM wave_state WHERE key = ?1", [key])?,
    };
    Ok(())
}

pub fn not_now(conn: &Connection, now_ms: i64) -> Result<Vec<NotNowRow>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT track_id, ctx_key, expires_at FROM feedback
         WHERE kind = 'not_now' AND track_id IS NOT NULL AND ctx_key IS NOT NULL AND expires_at > ?1",
    )?;
    let rows = st
        .query_map([now_ms], |r| {
            Ok(NotNowRow {
                track_id: r.get(0)?,
                ctx_key: r.get(1)?,
                expires_ms: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

impl WaveRepo for SqliteWaveRepo {
    fn record_play(&self, event: &PlayEvent) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        record_play(&conn, event).map_err(app_err)
    }

    fn plays_since(&self, since_ms: i64) -> Result<Vec<PlayEvent>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        plays_since(&conn, since_ms).map_err(app_err)
    }

    fn catalog(&self) -> Result<Vec<WaveTrack>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        catalog(&conn).map_err(app_err)
    }

    fn not_now(&self, now_ms: i64) -> Result<Vec<NotNowRow>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        not_now(&conn, now_ms).map_err(app_err)
    }

    fn feedback_since(&self, since_ms: i64) -> Result<Vec<FeedbackRow>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        feedback_since(&conn, since_ms).map_err(app_err)
    }

    fn state(&self, key: &str) -> Result<Option<String>, AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        state(&conn, key).map_err(app_err)
    }

    fn set_state(&self, key: &str, value: Option<&str>) -> Result<(), AppError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        set_state(&conn, key, value).map_err(app_err)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::repo::Identity;
    use crate::repo::{NewTrack, artist_get_or_create, track_insert, track_remove};

    pub(crate) fn new_track(path: &str, artist: i64) -> NewTrack {
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
            title: format!("Песня {path}"),
            album_id: None,
            disc_no: None,
            track_no: Some(1),
            year: None,
            identity: Identity::Inferred,
            identity_conf: 0.8,
            unknown_reason: None,
            artists: vec![artist],
        }
    }

    fn event(track_id: i64, ts: i64, reason: EndReason) -> PlayEvent {
        PlayEvent {
            ts,
            track_id,
            played_ms: 61_000,
            duration_ms: 180_000,
            end_reason: reason,
            source: PlaySource::Album,
            profile: None,
            context: ContextSnapshot {
                now_utc_ms: ts,
                local_minute: 19 * 60,
                weekday: 6,
                daypart: Daypart::Evening,
                season_w: [0.29, 0.0, 0.0, 0.71],
                weather: Some(WeatherClass::Snow),
            },
            event_id: None,
            novelty: None,
            category: None,
        }
    }

    #[test]
    fn journal_roundtrip_and_cascade() {
        let mut c = open_in_memory().unwrap();
        let a = artist_get_or_create(&c, "Кино").unwrap();
        let t1 = track_insert(&mut c, &new_track("a.flac", a)).unwrap();
        let t2 = track_insert(&mut c, &new_track("b.flac", a)).unwrap();
        record_play(&c, &event(t1, 1_000, EndReason::Finished)).unwrap();
        record_play(&c, &event(t2, 2_000, EndReason::Skip)).unwrap();
        let all = plays_since(&c, 0).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].track_id, t1);
        assert_eq!(all[1].end_reason, EndReason::Skip);
        assert_eq!(all[0].context.daypart, Daypart::Evening);
        assert_eq!(all[0].context.weather, Some(WeatherClass::Snow));
        assert!((all[0].context.season_w[3] - 0.71).abs() < 0.01);
        assert_eq!(plays_since(&c, 1_500).unwrap().len(), 1);
        // A removed track takes its journal with it (foreign keys are on).
        track_remove(&mut c, t1).unwrap();
        let left = plays_since(&c, 0).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].track_id, t2);
    }

    #[test]
    fn unknown_track_is_rejected() {
        let c = open_in_memory().unwrap();
        assert!(record_play(&c, &event(999, 1, EndReason::Finished)).is_err());
    }

    #[test]
    fn repo_trait_over_a_connection() {
        let mut c = open_in_memory().unwrap();
        let a = artist_get_or_create(&c, "Сплин").unwrap();
        let t = track_insert(&mut c, &new_track("c.flac", a)).unwrap();
        let repo = SqliteWaveRepo::from_connection(c);
        repo.record_play(&event(t, 5, EndReason::Quit)).unwrap();
        assert_eq!(repo.plays_since(0).unwrap()[0].end_reason, EndReason::Quit);
    }
}
