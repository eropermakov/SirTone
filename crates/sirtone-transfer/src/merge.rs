//! How a package merges into a library (D-140). Pure decisions over plain data; the app applies
//! them through its own repository (on the PC file moves go through `fs_journal`).

use std::collections::{HashMap, HashSet};

use sirtone_core::fingerprint::similarity;
use sirtone_core::text::norm;

use crate::{Event, EventKind, TrackEntry};

/// «Тот же звук» — the thresholds of the library's duplicate finder (S-06, `sirtone-library`
/// `duplicates`): similarity ≥ 0.92 over ≥ 80 % of the shorter track, |Δ duration| ≤ 3 s.
pub const SAME_SOUND: f32 = 0.92;
pub const MIN_OVERLAP: f32 = 0.8;
pub const MAX_DURATION_DELTA_MS: u32 = 3_000;
/// Without fingerprints: same normalised artist and title and |Δ duration| ≤ 2 s.
pub const TAG_DURATION_DELTA_MS: u32 = 2_000;

/// A track already in the destination library.
#[derive(Debug, Clone)]
pub struct LocalTrack {
    pub id: i64,
    pub duration_ms: u32,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub fingerprint: Option<Vec<u32>>,
    /// BLAKE3 of the file, when known.
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackMatch {
    /// The destination has this sound: no copy, statistics and marks go to `local id`.
    Same(i64),
    /// New music: copy the file, import it (recognition, layout) as any import.
    New,
}

fn tag_key(artist: Option<&str>, title: Option<&str>) -> Option<(String, String)> {
    Some((norm(artist?)?, norm(title?)?))
}

/// Matches every package track with the destination library: the same file (content hash) first,
/// then by sound when both sides have a fingerprint, else by artist + title + duration. Candidates are narrowed by duration first, so
/// 10 000 × 10 000 stays cheap.
pub fn match_tracks(incoming: &[TrackEntry], local: &[LocalTrack]) -> Vec<TrackMatch> {
    let mut by_duration: Vec<&LocalTrack> = local.iter().collect();
    by_duration.sort_by_key(|t| t.duration_ms);
    let durations: Vec<u32> = by_duration.iter().map(|t| t.duration_ms).collect();
    let tags: HashMap<(String, String), Vec<&LocalTrack>> =
        local.iter().fold(HashMap::new(), |mut m, t| {
            if let Some(k) = tag_key(t.artist.as_deref(), t.title.as_deref()) {
                m.entry(k).or_default().push(t);
            }
            m
        });

    let hashes: HashMap<&str, i64> = local
        .iter()
        .filter_map(|t| Some((t.hash.as_deref()?, t.id)))
        .collect();
    incoming
        .iter()
        .map(|t| {
            // The very same file (a package applied after an import, an untagged file).
            if let Some(&id) = t.hash.as_deref().and_then(|h| hashes.get(h)) {
                return TrackMatch::Same(id);
            }
            if let Some(fp) = &t.fingerprint {
                let lo = durations.partition_point(|&d| d + MAX_DURATION_DELTA_MS < t.duration_ms);
                let hi = durations
                    .partition_point(|&d| d <= t.duration_ms.saturating_add(MAX_DURATION_DELTA_MS));
                let best = by_duration[lo..hi]
                    .iter()
                    .filter_map(|l| {
                        let (sim, overlap) = similarity(fp, l.fingerprint.as_deref()?)?;
                        (sim >= SAME_SOUND && overlap >= MIN_OVERLAP).then_some((sim, l.id))
                    })
                    .max_by(|a, b| a.0.total_cmp(&b.0));
                if let Some((_, id)) = best {
                    return TrackMatch::Same(id);
                }
            }
            tag_key(t.artist.as_deref(), t.title.as_deref())
                .and_then(|k| tags.get(&k))
                .and_then(|c| {
                    c.iter()
                        .filter(|l| l.duration_ms.abs_diff(t.duration_ms) <= TAG_DURATION_DELTA_MS)
                        .min_by_key(|l| l.duration_ms.abs_diff(t.duration_ms))
                })
                .map_or(TrackMatch::New, |l| TrackMatch::Same(l.id))
        })
        .collect()
}

/// Events not applied yet: keys already seen by the destination (its own journal keys and keys
/// of earlier transfers) are dropped, and so are duplicates within the package.
pub fn new_events<'a>(seen: &HashSet<String>, incoming: &'a [Event]) -> Vec<&'a Event> {
    let mut taken = HashSet::new();
    incoming
        .iter()
        .filter(|e| !seen.contains(&e.key) && taken.insert(e.key.as_str()))
        .collect()
}

/// ♡ after the merge (D-140 «побеждает последняя отметка»): for each track the latest of the
/// local mark and the package's `like`/`unlike` marks. Returns `track_key → (liked, ts)` for the
/// tracks whose state the package changes.
pub fn likes_after_merge(
    local: &HashMap<String, (bool, i64)>,
    incoming: &[Event],
) -> HashMap<String, (bool, i64)> {
    let mut latest: HashMap<String, (bool, i64)> = HashMap::new();
    for e in incoming {
        let EventKind::Mark { kind } = &e.kind else {
            continue;
        };
        let liked = match kind.as_str() {
            "like" => true,
            "unlike" => false,
            _ => continue,
        };
        let slot = latest.entry(e.track_key.clone()).or_insert((liked, e.ts));
        if e.ts >= slot.1 {
            *slot = (liked, e.ts);
        }
    }
    latest
        .into_iter()
        .filter(|(track, (liked, ts))| match local.get(track) {
            Some((l, lts)) => ts > lts && liked != l,
            None => *liked,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(seed: u32, n: usize) -> Vec<u32> {
        // A deterministic pseudo-random fingerprint (xorshift).
        let mut x = seed | 1;
        (0..n)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x
            })
            .collect()
    }

    fn entry(key: &str, dur: u32, artist: &str, title: &str, f: Option<Vec<u32>>) -> TrackEntry {
        TrackEntry {
            key: key.into(),
            file: format!("music/{key}.flac"),
            size: 1,
            duration_ms: dur,
            artist: Some(artist.into()),
            title: Some(title.into()),
            album: None,
            fingerprint: f,
            hash: None,
            copy: None,
        }
    }

    fn local(id: i64, dur: u32, artist: &str, title: &str, f: Option<Vec<u32>>) -> LocalTrack {
        LocalTrack {
            id,
            duration_ms: dur,
            artist: Some(artist.into()),
            title: Some(title.into()),
            fingerprint: f,
            hash: None,
        }
    }

    #[test]
    fn tracks_match_by_sound_then_tags() {
        let lib = vec![
            local(1, 200_000, "Кино", "Группа крови", Some(fp(7, 900))),
            local(2, 180_000, "Ария", "Беспечный ангел", None),
            local(3, 240_000, "Другое", "Другое", Some(fp(99, 900))),
        ];
        let mut retagged = fp(7, 900);
        retagged[10] ^= 0xFF; // a few bits differ: another rip of the same sound
        let pkg = vec![
            // Same sound under other tags, 1.5 s longer.
            entry("a", 201_500, "KINO", "Gruppa krovi", Some(retagged)),
            // No fingerprint: tags with ё/case/dash differences, 1 s apart.
            entry("b", 181_000, "АРИЯ", "Беспечный — ангел", None),
            // Same tags but 10 s longer: another version → new.
            entry("c", 190_000, "Ария", "Беспечный ангел", None),
            // Other sound with the same duration → new.
            entry("d", 240_000, "Кто-то", "Что-то", Some(fp(5, 900))),
        ];
        assert_eq!(
            match_tracks(&pkg, &lib),
            vec![
                TrackMatch::Same(1),
                TrackMatch::Same(2),
                TrackMatch::New,
                TrackMatch::New
            ]
        );
    }

    fn mark(key: &str, track: &str, ts: i64, kind: &str) -> Event {
        Event {
            key: key.into(),
            track_key: track.into(),
            ts,
            kind: EventKind::Mark { kind: kind.into() },
        }
    }

    #[test]
    fn events_apply_once() {
        let pkg = vec![
            mark("pc:mark:1", "t1", 1, "like"),
            mark("pc:mark:2", "t1", 2, "unlike"),
            mark("pc:mark:2", "t1", 2, "unlike"),
        ];
        let seen: HashSet<String> = ["pc:mark:1".to_string()].into();
        let fresh = new_events(&seen, &pkg);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].key, "pc:mark:2");
    }

    #[test]
    fn latest_like_wins() {
        let local: HashMap<String, (bool, i64)> =
            [("t1".into(), (true, 50)), ("t2".into(), (false, 50))].into();
        let pkg = vec![
            mark("k1", "t1", 40, "unlike"), // older than the local ♡ → ignored
            mark("k2", "t2", 60, "like"),   // newer → ♡ on
            mark("k3", "t3", 10, "like"),   // unknown locally → ♡ on
            mark("k4", "t4", 10, "unlike"), // nothing to switch off
            mark("k5", "t2", 55, "unlike"), // older than k2 inside the package
        ];
        let out = likes_after_merge(&local, &pkg);
        assert_eq!(out.len(), 2);
        assert_eq!(out["t2"], (true, 60));
        assert_eq!(out["t3"], (true, 10));
    }
}
