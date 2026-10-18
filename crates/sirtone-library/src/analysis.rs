//! Storage of analysis results (T3.6/T3.10): the queue of tracks still to analyse and the B0
//! columns of `track` (docs/03 §7). Files are only read, never changed.

use rusqlite::{Connection, params};
use sirtone_core::analysis::B0;

use crate::db::now_ms;
use crate::error::LibraryError;
use crate::repo::TrackId;

/// Playable tracks whose analysis is older than `version`, newest first.
pub fn pending(
    conn: &Connection,
    version: i64,
    limit: u32,
) -> Result<Vec<(TrackId, String)>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT id, rel_path FROM track WHERE state = 'ok' AND analysis_version < ?1
         ORDER BY added_at DESC, id DESC LIMIT ?2",
    )?;
    let rows = st
        .query_map(params![version, limit], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// How many tracks are analysed / in total (progress in Settings).
pub fn progress(conn: &Connection, version: i64) -> Result<(u32, u32), LibraryError> {
    Ok(conn.query_row(
        "SELECT coalesce(sum(analysis_version >= ?1), 0), count(*) FROM track WHERE state = 'ok'",
        [version],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

pub fn store_b0(
    conn: &Connection,
    track: TrackId,
    b: &B0,
    version: i64,
) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE track SET lufs_i = ?2, true_peak_db = ?3, lra = ?4, peaks = ?5,
                lead_silence_ms = ?6, tail_silence_ms = ?7, env_head = ?8, env_tail = ?9,
                analysis_version = ?10, analyzed_at = ?11, fingerprint = ?12, fp_duration_ms = ?13
         WHERE id = ?1",
        params![
            track,
            b.lufs_i,
            b.true_peak_db,
            b.lra,
            b.peaks,
            b.lead_silence_ms as i64,
            b.tail_silence_ms as i64,
            b.env_head,
            b.env_tail,
            version,
            now_ms(),
            b.fingerprint,
            b.fingerprint.as_ref().map(|_| b.fp_duration_ms as i64),
        ],
    )?;
    Ok(())
}

/// A file that cannot be analysed is not retried at this version (it stays playable).
pub fn mark_failed(conn: &Connection, track: TrackId, version: i64) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE track SET analysis_version = ?2, analyzed_at = ?3 WHERE id = ?1",
        params![track, version, now_ms()],
    )?;
    Ok(())
}
