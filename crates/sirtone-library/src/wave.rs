//! Wave data in `library.db` (docs/14 §18, T1.30): the listening journal. The repository keeps
//! its own connection (WAL lets it write next to the import worker); each event is one
//! transaction, so a crash loses at most the event in flight (T1.31).

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, params};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::wave::{
    ContextSnapshot, Daypart, EndReason, PlayEvent, PlaySource, WaveRepo, WeatherClass,
    season_w_bytes,
};

use crate::db::{self, Medium};
use crate::error::LibraryError;

pub fn record_play(conn: &Connection, e: &PlayEvent) -> Result<(), LibraryError> {
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
