//! What the automix planner needs about a track (T4.9, `04` §4.2): duration, silences, loudness,
//! the 10 Hz envelopes of the first and last 30 s, genre, tempo, key, energy — as stored by the
//! analysis (B0/B1/B3). The player turns it into the planner's own type.

use rusqlite::{Connection, OptionalExtension};

use crate::error::LibraryError;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MixInfo {
    pub duration_ms: u64,
    pub lead_silence_ms: Option<u64>,
    pub tail_silence_ms: Option<u64>,
    pub lufs: Option<f32>,
    pub env_head: Vec<u8>,
    pub env_tail: Vec<u8>,
    pub genre: Option<String>,
    pub bpm: Option<f32>,
    pub camelot: Option<String>,
    pub energy: Option<f32>,
}

pub fn of_track(conn: &Connection, id: i64) -> Result<Option<MixInfo>, LibraryError> {
    Ok(conn
        .prepare_cached(
            "SELECT duration_ms, lead_silence_ms, tail_silence_ms, lufs_i, env_head, env_tail,
                    genre_main, bpm, key_camelot, energy
               FROM track WHERE id = ?1",
        )?
        .query_row([id], |r| {
            Ok(MixInfo {
                duration_ms: r.get::<_, i64>(0)?.max(0) as u64,
                lead_silence_ms: r.get::<_, Option<i64>>(1)?.map(|v| v.max(0) as u64),
                tail_silence_ms: r.get::<_, Option<i64>>(2)?.map(|v| v.max(0) as u64),
                lufs: r.get::<_, Option<f64>>(3)?.map(|v| v as f32),
                env_head: r.get::<_, Option<Vec<u8>>>(4)?.unwrap_or_default(),
                env_tail: r.get::<_, Option<Vec<u8>>>(5)?.unwrap_or_default(),
                genre: r.get(6)?,
                bpm: r.get::<_, Option<f64>>(7)?.map(|v| v as f32),
                camelot: r.get(8)?,
                energy: r.get::<_, Option<f64>>(9)?.map(|v| v as f32),
            })
        })
        .optional()?)
}
