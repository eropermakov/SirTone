//! Listening statistics (T3.17, FR-PLAY-015; `05` §11, `01b` F-20): the Last.fm rule decides
//! what counts — a track longer than 30 s heard for half its length or 4 minutes, whichever
//! comes first; leaving earlier with «next» is a skip (stop, quit and errors are neither).
//! Aggregates live in `track` (counts, last played, completion EMA, 13 weekly buckets) for
//! lists and smart rules; the full journal stays in `play_event` for the wave.

use rusqlite::{Connection, OptionalExtension, params};
use sirtone_core::wave::{EndReason, PlayEvent};

use crate::error::LibraryError;
use crate::repo::{TrackId, TrackRow};

/// Last.fm: only tracks longer than this count at all.
const MIN_TRACK_MS: u64 = 30_000;
/// Last.fm: 4 minutes are always enough.
const ENOUGH_MS: u64 = 240_000;
/// Weight of the newest play in `completion_ema`.
const EMA_ALPHA: f64 = 0.2;
const WEEK_MS: i64 = 7 * 24 * 3600 * 1000;
pub const WEEKS: usize = 13;

/// Does this listen count as a play (Last.fm)?
pub fn counts(played_ms: u64, duration_ms: u64) -> bool {
    duration_ms > MIN_TRACK_MS && played_ms >= (duration_ms / 2).min(ENOUGH_MS)
}

/// A skip: «next» before the play counted.
pub fn is_skip(e: &PlayEvent) -> bool {
    e.end_reason == EndReason::Skip && !counts(e.played_ms, e.duration_ms)
}

/// Shifts the weekly ring from the week of `last_ms` to `now_ms` (old weeks become 0) and
/// adds one play to the current week. Bucket `i` holds week `(now_week − i)`.
pub fn bump_weeks(buckets: &[u8], last_ms: Option<i64>, now_ms: i64) -> [u8; WEEKS] {
    let mut out = [0u8; WEEKS];
    let now_w = now_ms.div_euclid(WEEK_MS);
    if let Some(last) = last_ms {
        let shift = (now_w - last.div_euclid(WEEK_MS)).max(0) as usize;
        for (i, b) in buckets.iter().take(WEEKS).enumerate() {
            if i + shift < WEEKS {
                out[i + shift] = *b;
            }
        }
    }
    out[0] = out[0].saturating_add(1);
    out
}

/// Applies one journal event to the track aggregates (same transaction as the journal row).
pub fn apply(conn: &Connection, e: &PlayEvent) -> Result<(), LibraryError> {
    let Some((last, buckets, ema)): Option<(Option<i64>, Option<Vec<u8>>, f64)> = conn
        .query_row(
            "SELECT last_played_at, plays_recent, completion_ema FROM track WHERE id = ?1",
            [e.track_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
    else {
        return Ok(());
    };
    // Completion only from listens that ended by the music or by the listener.
    let ema = if matches!(
        e.end_reason,
        EndReason::Finished | EndReason::MixedOut | EndReason::Skip
    ) && e.duration_ms > 0
    {
        // Mixed out by automix: the overlap cut the end on purpose — a complete listen.
        let frac = if e.end_reason == EndReason::MixedOut {
            1.0
        } else {
            (e.played_ms as f64 / e.duration_ms as f64).min(1.0)
        };
        (1.0 - EMA_ALPHA) * ema + EMA_ALPHA * frac
    } else {
        ema
    };
    if counts(e.played_ms, e.duration_ms) {
        let weeks = bump_weeks(buckets.as_deref().unwrap_or(&[]), last, e.ts);
        conn.execute(
            "UPDATE track SET play_count = play_count + 1, last_played_at = ?2,
                    plays_recent = ?3, completion_ema = ?4 WHERE id = ?1",
            params![e.track_id, e.ts, weeks.as_slice(), ema],
        )?;
    } else if is_skip(e) {
        conn.execute(
            "UPDATE track SET skip_count = skip_count + 1, completion_ema = ?2 WHERE id = ?1",
            params![e.track_id, ema],
        )?;
    } else {
        conn.execute(
            "UPDATE track SET completion_ema = ?2 WHERE id = ?1",
            params![e.track_id, ema],
        )?;
    }
    Ok(())
}

/// «Часто слушаю»: plays in the last 13 weeks, most first.
pub fn often(conn: &Connection, limit: u32) -> Result<Vec<(TrackId, u32)>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT id, plays_recent FROM track WHERE state = 'ok' AND plays_recent IS NOT NULL",
    )?;
    let mut rows: Vec<(TrackId, u32)> = st
        .query_map([], |r| {
            let b: Vec<u8> = r.get(1)?;
            Ok((r.get(0)?, b.iter().map(|&x| u32::from(x)).sum()))
        })?
        .collect::<Result<_, _>>()?;
    rows.retain(|(_, n)| *n > 0);
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    rows.truncate(limit as usize);
    Ok(rows)
}

/// «Забытые»: played often before (≥ `min_plays`) but not for `days`.
pub fn forgotten(
    conn: &Connection,
    now_ms: i64,
    days: u32,
    min_plays: u32,
    limit: u32,
) -> Result<Vec<TrackRow>, LibraryError> {
    let cutoff = now_ms - i64::from(days) * 24 * 3600 * 1000;
    let mut st = conn.prepare(&format!(
        "SELECT {} FROM track t LEFT JOIN album al ON al.id = t.album_id
         WHERE t.state = 'ok' AND t.play_count >= ?1 AND t.last_played_at < ?2
         ORDER BY t.play_count DESC, t.last_played_at LIMIT ?3",
        crate::collection::TRACK_COLS
    ))?;
    let rows = st
        .query_map(
            params![min_plays, cutoff, limit],
            crate::collection::track_row,
        )?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_fm_rule() {
        assert!(!counts(30_000, 30_000), "30 s tracks never count");
        assert!(counts(16_000, 31_000), "half of a 31 s track");
        assert!(!counts(15_000, 31_000));
        assert!(counts(240_000, 600_000), "4 minutes of a 10 minute track");
        assert!(!counts(239_000, 600_000));
        assert!(counts(90_000, 180_000));
    }

    #[test]
    fn weekly_ring_shifts_and_counts() {
        let w0 = 1_000 * WEEK_MS;
        let a = bump_weeks(&[], None, w0);
        assert_eq!(a[0], 1);
        let b = bump_weeks(&a, Some(w0), w0 + 1000);
        assert_eq!(b[0], 2, "same week");
        let c = bump_weeks(&b, Some(w0), w0 + 2 * WEEK_MS);
        assert_eq!(
            (c[0], c[2]),
            (1, 2),
            "two weeks later the old plays moved to bucket 2"
        );
        let d = bump_weeks(&c, Some(w0 + 2 * WEEK_MS), w0 + 40 * WEEK_MS);
        assert_eq!(
            d.iter().map(|&x| u32::from(x)).sum::<u32>(),
            1,
            "older than 13 weeks drop"
        );
    }

    #[test]
    fn journal_rows_update_the_aggregates() {
        use crate::db::open;
        use crate::repo::{Identity, NewTrack, track_insert};
        use sirtone_core::wave::{ContextSnapshot, Daypart, PlaySource};
        let root = std::env::temp_dir().join(format!("sirtone-stats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut c = open(&root.join(".sirtone/library.db"), crate::Medium::Fixed).unwrap();
        let t = track_insert(
            &mut c,
            &NewTrack {
                rel_path: "a.mp3".into(),
                size_bytes: 5,
                mtime_ms: 0,
                content_hash: "h".into(),
                container: "mp3".into(),
                codec: "mp3".into(),
                lossless: false,
                sample_rate: None,
                channels: None,
                duration_ms: 200_000,
                title: "A".into(),
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
        let ev = |played: u64, reason: EndReason, ts: i64| PlayEvent {
            ts,
            track_id: t,
            played_ms: played,
            duration_ms: 200_000,
            end_reason: reason,
            source: PlaySource::Collection,
            profile: None,
            context: ContextSnapshot {
                now_utc_ms: ts,
                local_minute: 600,
                weekday: 0,
                daypart: Daypart::Day,
                season_w: [0.0, 0.0, 1.0, 0.0],
                weather: None,
            },
            event_id: None,
            novelty: None,
            category: None,
        };
        crate::wave::record_play(&c, &ev(200_000, EndReason::Finished, 1_000)).unwrap();
        crate::wave::record_play(&c, &ev(20_000, EndReason::Skip, 2_000)).unwrap();
        crate::wave::record_play(&c, &ev(5_000, EndReason::Stop, 3_000)).unwrap();
        let (plays, skips, last, ema): (i64, i64, i64, f64) = c
            .query_row(
                "SELECT play_count, skip_count, last_played_at, completion_ema FROM track WHERE id = ?1",
                [t],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (plays, skips, last),
            (1, 1, 1_000),
            "stop is neither a play nor a skip"
        );
        assert!((ema - (0.2 * 0.8 + 0.2 * 0.1)).abs() < 1e-9, "{ema}");
        assert_eq!(often(&c, 10).unwrap(), vec![(t, 1)]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
