//! `events.jsonl`: the listening journal and the marks, one JSON object per line.
//!
//! Every event has a global `key` (`<library_uuid>:<kind>:<local id>`), so a package applied
//! twice — or a journal that already travelled the other way — never doubles anything.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::TransferError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    /// A play (`play_event`): what the wave learns from.
    Play {
        played_ms: u32,
        duration_ms: u32,
        /// `finished | mixed_out | skip | sleep | stop | quit | error` (as in the library).
        end_reason: String,
        /// `wave | search | …` (as in the library).
        source: String,
    },
    /// A mark (`feedback`): `like | unlike | not_now | never | unnever`.
    Mark { kind: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub key: String,
    /// `TrackEntry::key` of the track, or a key of a track the other side already has.
    pub track_key: String,
    /// Unix ms UTC.
    pub ts: i64,
    #[serde(flatten)]
    pub kind: EventKind,
}

pub fn write_jsonl(out: &mut impl Write, events: &[Event]) -> Result<(), TransferError> {
    for e in events {
        serde_json::to_writer(&mut *out, e)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

/// Reads the journal. A last line cut by a broken transfer is ignored; any other broken line
/// fails the read (the package is damaged, not merely incomplete).
pub fn read_jsonl(input: impl BufRead) -> Result<Vec<Event>, TransferError> {
    let lines: Vec<String> = input.lines().collect::<Result<_, _>>()?;
    let mut out = Vec::with_capacity(lines.len());
    let last = lines.iter().rposition(|l| !l.trim().is_empty());
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(line) {
            Ok(e) => out.push(e),
            Err(_) if Some(i) == last => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(key: &str) -> Event {
        Event {
            key: key.into(),
            track_key: "lib-pc:1".into(),
            ts: 1,
            kind: EventKind::Play {
                played_ms: 170_000,
                duration_ms: 180_000,
                end_reason: "finished".into(),
                source: "wave".into(),
            },
        }
    }

    #[test]
    fn round_trip_and_cut_tail() {
        let events = vec![
            play("lib-pc:play:1"),
            Event {
                key: "lib-pc:mark:1".into(),
                track_key: "lib-pc:1".into(),
                ts: 2,
                kind: EventKind::Mark {
                    kind: "like".into(),
                },
            },
        ];
        let mut buf = Vec::new();
        write_jsonl(&mut buf, &events).unwrap();
        assert_eq!(read_jsonl(&buf[..]).unwrap(), events);
        // A transfer cut in the middle of the last line.
        buf.extend_from_slice(br#"{"key":"lib-pc:play:2","track_"#);
        assert_eq!(read_jsonl(&buf[..]).unwrap(), events);
        // A broken line in the middle is damage, not a cut.
        let mut bad = b"{oops}\n".to_vec();
        write_jsonl(&mut bad, &events).unwrap();
        assert!(read_jsonl(&bad[..]).is_err());
    }
}
