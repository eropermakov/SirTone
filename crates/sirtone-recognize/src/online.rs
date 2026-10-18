//! Online add-on without an AcoustID key (2.0, D-84, D-87; `05` §4.6; `01b` F-07): a
//! MusicBrainz recording search by what the library already knows — like Picard's «Lookup»
//! (`lookup_metadata` → `find_tracks`, S140) and beets' recording search (S141) — then the
//! same weighted distance and three zones as offline (`05` §4.4).
//!
//! A text match is weaker than a fingerprint, so applying it automatically needs all of: the
//! artist and the title agree (≥ 0.9), the length is within 2 s, and no recording of another
//! artist is just as close. Anything less is a hint for «Неизвестные». Album: the local hint
//! first, else the earliest official album (no compilations, live, soundtracks) among the
//! matching recordings, else none («Singles», `05` §4.6 c). Year: only from the release group
//! (one more request), never from a reissue.

use serde_json::Value;

use crate::identify::similarity;

/// What the library knows about a track.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Hints {
    pub title: String,
    /// Primary artist.
    pub artist: String,
    /// A local album (tags, folder consensus) wins over MusicBrainz's choice.
    pub album: Option<String>,
    pub duration_ms: u64,
}

/// One MusicBrainz recording against the hints.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub mbid: String,
    pub title: String,
    pub artists: Vec<String>,
    pub length_ms: Option<u64>,
    /// Weighted distance (`05` §4.4): artist 3, title 3, length 2.
    pub d: f64,
}

/// What to apply for a confirmed match.
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub mbid: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    /// The release group of a MusicBrainz album (its first release year is asked separately).
    pub release_group: Option<String>,
    pub track_no: Option<u32>,
    /// 1 − d.
    pub confidence: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Apply: the file moves to its place.
    Confirmed(Match),
    /// Up to three candidates for «Неизвестные».
    Hint(Vec<Candidate>),
    Nothing,
}

const AGREE: f64 = 0.9;
const LENGTH_OK_MS: u64 = 2_000;
const LENGTH_FAR_MS: u64 = 10_000;
const HINT_D: f64 = 0.25;
const RESULTS: u32 = 25;

/// Lucene escaping as Picard does it; lower case keeps «AND»/«OR»/«NOT» in a title from
/// becoming operators (beets lowercases too).
fn term(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 8);
    for c in v.trim().to_lowercase().chars() {
        if "+-&|!(){}[]^\"~*?:\\/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The search path under the MusicBrainz base (`ws/2/`), form-encoded.
pub fn search_path(h: &Hints, encode: impl Fn(&str) -> String) -> String {
    let mut q = format!(
        "recording:({}) AND artist:({})",
        term(&h.title),
        term(&h.artist)
    );
    if h.duration_ms > 0 {
        // Picard's `qdur` (length / 2000) favours the right version.
        q.push_str(&format!(" qdur:({})", h.duration_ms / 2000));
    }
    format!("recording?query={}&fmt=json&limit={RESULTS}", encode(&q))
}

/// The release group path for the year of an album.
pub fn release_group_path(rgid: &str) -> Option<String> {
    let ok = rgid.len() == 36 && rgid.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    ok.then(|| format!("release-group/{rgid}?fmt=json"))
}

/// The first release year of a release group answer.
pub fn release_group_year(json: &[u8]) -> Option<i32> {
    let v: Value = serde_json::from_slice(json).ok()?;
    v["first-release-date"].as_str()?.get(..4)?.parse().ok()
}

fn length_distance(a: u64, b: u64) -> f64 {
    let diff = a.abs_diff(b);
    if diff <= LENGTH_OK_MS {
        0.0
    } else {
        ((diff - LENGTH_OK_MS) as f64 / (LENGTH_FAR_MS - LENGTH_OK_MS) as f64).min(1.0)
    }
}

struct Rec<'a> {
    v: &'a Value,
    c: Candidate,
    artist_sim: f64,
    title_sim: f64,
    length_ok: bool,
}

/// Decides on a search answer.
pub fn decide(json: &[u8], h: &Hints) -> Result<Verdict, String> {
    let v: Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    let recs: Vec<Rec<'_>> = v["recordings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let mbid = r["id"].as_str()?.to_string();
            let title = r["title"].as_str()?.to_string();
            let artists: Vec<String> = r["artist-credit"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["name"].as_str().map(str::to_string))
                .collect();
            if artists.is_empty() {
                return None;
            }
            let length_ms = r["length"].as_u64();
            let artist_sim = artists
                .iter()
                .map(|a| similarity(a, &h.artist))
                .chain(std::iter::once(similarity(&artists.join(" "), &h.artist)))
                .fold(0.0, f64::max);
            let title_sim = similarity(&title, &h.title);
            let len_d = match (length_ms, h.duration_ms) {
                (Some(a), b) if b > 0 => Some(length_distance(a, b)),
                _ => None,
            };
            let (mut num, mut den) = (3.0 * (1.0 - artist_sim) + 3.0 * (1.0 - title_sim), 6.0);
            if let Some(l) = len_d {
                num += 2.0 * l;
                den += 2.0;
            }
            Some(Rec {
                v: r,
                c: Candidate {
                    mbid,
                    title,
                    artists,
                    length_ms,
                    d: num / den,
                },
                artist_sim,
                title_sim,
                length_ok: len_d == Some(0.0),
            })
        })
        .collect();
    let confirmed: Vec<&Rec<'_>> = recs
        .iter()
        .filter(|r| r.artist_sim >= AGREE && r.title_sim >= AGREE && r.length_ok)
        .collect();
    let rival = recs
        .iter()
        .any(|r| r.title_sim >= AGREE && r.length_ok && r.artist_sim < AGREE);
    if let Some(best) = confirmed
        .iter()
        .min_by(|a, b| a.c.d.total_cmp(&b.c.d).then(a.c.mbid.cmp(&b.c.mbid)))
        && !rival
    {
        let (album, release_group, track_no) = match &h.album {
            Some(a) => (Some(a.clone()), None, None),
            None => album_of(&confirmed),
        };
        return Ok(Verdict::Confirmed(Match {
            mbid: best.c.mbid.clone(),
            title: best.c.title.clone(),
            artists: best.c.artists.clone(),
            album,
            release_group,
            track_no,
            confidence: 1.0 - best.c.d,
        }));
    }
    let mut hints: Vec<Candidate> = recs
        .into_iter()
        .filter(|r| r.c.d <= HINT_D)
        .map(|r| r.c)
        .collect();
    hints.sort_by(|a, b| a.d.total_cmp(&b.d).then(a.mbid.cmp(&b.mbid)));
    hints.dedup_by(|a, b| {
        a.title == b.title && a.artists == b.artists && a.length_ms == b.length_ms
    });
    hints.truncate(3);
    Ok(if hints.is_empty() {
        Verdict::Nothing
    } else {
        Verdict::Hint(hints)
    })
}

/// The earliest official album without secondary types among the matching recordings: its
/// title, release group and the track's number on it.
fn album_of(recs: &[&Rec<'_>]) -> (Option<String>, Option<String>, Option<u32>) {
    let mut best: Option<(String, String, String, Option<u32>)> = None;
    for r in recs {
        for rel in r.v["releases"].as_array().into_iter().flatten() {
            let rg = &rel["release-group"];
            let album = rel["status"].as_str() == Some("Official")
                && rg["primary-type"].as_str() == Some("Album")
                && rg["secondary-types"]
                    .as_array()
                    .is_none_or(|s| s.is_empty());
            let (Some(title), Some(rgid)) = (rel["title"].as_str(), rg["id"].as_str()) else {
                continue;
            };
            if !album {
                continue;
            }
            // Undated releases sort last.
            let date = rel["date"].as_str().unwrap_or("9999").to_string();
            let track_no = rel["media"]
                .as_array()
                .and_then(|m| m.first())
                .and_then(|m| m["track"].as_array())
                .and_then(|t| t.first())
                .and_then(|t| t["number"].as_str())
                .and_then(|n| n.parse().ok());
            if best.as_ref().is_none_or(|b| date < b.0) {
                best = Some((date, title.to_string(), rgid.to_string(), track_no));
            }
        }
    }
    match best {
        Some((_, t, rg, n)) => (Some(t), Some(rg), n),
        None => (None, None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> Vec<u8> {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name);
        std::fs::read(p).expect("fixture")
    }

    #[test]
    fn queries_are_escaped_and_lowercased() {
        let h = Hints {
            title: "Love AND Hate (Live)".into(),
            artist: "AC/DC".into(),
            album: None,
            duration_ms: 241_000,
        };
        let p = search_path(&h, |s| s.to_string());
        assert_eq!(
            p,
            "recording?query=recording:(love and hate \\(live\\)) AND artist:(ac\\/dc) qdur:(120)&fmt=json&limit=25"
        );
        assert_eq!(release_group_path("../x"), None);
        assert!(release_group_path("7ff1eff0-3b5c-3a3b-9a6f-7a7bd9c30a0e").is_some());
    }

    /// Кино «Группа крови» 4:46 (MusicBrainz answer, 2026-10-19): the album version is
    /// confirmed, the album is the earliest official album among the matches.
    #[test]
    fn kino_confirmed_with_its_album() {
        let h = Hints {
            title: "Группа крови".into(),
            artist: "Кино".into(),
            album: None,
            duration_ms: 286_500,
        };
        let Verdict::Confirmed(m) = decide(&data("mb_kino.json"), &h).expect("parse") else {
            panic!("not confirmed");
        };
        assert_eq!(m.title, "Группа крови");
        assert_eq!(m.artists, vec!["Кино".to_string()]);
        assert_eq!(m.album.as_deref(), Some("Группа крови"));
        assert!(m.release_group.is_some());
        assert!(m.confidence > 0.95);
    }

    #[test]
    fn a_local_album_wins_and_a_wrong_length_is_only_a_hint() {
        let mut h = Hints {
            title: "Creep".into(),
            artist: "Radiohead".into(),
            album: Some("Pablo Honey".into()),
            duration_ms: 238_640,
        };
        let Verdict::Confirmed(m) = decide(&data("mb_creep.json"), &h).expect("parse") else {
            panic!("not confirmed");
        };
        assert_eq!(m.album.as_deref(), Some("Pablo Honey"));
        // Nothing within 2 s: the closest versions are only offered.
        h.duration_ms = 200_000;
        assert!(matches!(
            decide(&data("mb_creep.json"), &h),
            Ok(Verdict::Hint(_))
        ));
        // Another artist with this title: nothing to say.
        h.artist = "Земфира".into();
        h.duration_ms = 238_640;
        assert_eq!(decide(&data("mb_creep.json"), &h), Ok(Verdict::Nothing));
    }

    #[test]
    fn release_group_year_and_bad_json() {
        assert_eq!(
            release_group_year(br#"{"first-release-date":"1988-01-05"}"#),
            Some(1988)
        );
        assert_eq!(release_group_year(b"{}"), None);
        assert!(decide(b"not json", &Hints::default()).is_err());
        assert_eq!(
            decide(br#"{"recordings":[]}"#, &Hints::default()),
            Ok(Verdict::Nothing)
        );
    }
}
