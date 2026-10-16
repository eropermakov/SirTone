//! Offline identity decision, first version (T1.15 + FR-IMP-007): tags only. The file-name parser,
//! folder consensus and the distance/zone table (docs/05 §4.3–4.5) replace the confidence rule in
//! T1.16–T1.17; the interface stays.

use sirtone_core::ports::{Identify, IdentityDecision, MediaInfo, Tags, UnknownReason};

/// Separators between artists (docs/05 §4.2), tried case-insensitively.
const SEPARATORS: &[&str] = &[
    " feat. ",
    " feat ",
    " ft. ",
    " ft ",
    " featuring ",
    " & ",
    ", ",
    " x ",
    " vs. ",
    " vs ",
    " / ",
    " и ",
    " with ",
];

/// «Би-2 feat. Земфира» → ["Би-2", "Земфира"]; the first one is the primary artist.
pub fn split_artists(s: &str) -> Vec<String> {
    let mut parts = vec![s.to_string()];
    for sep in SEPARATORS {
        parts = parts
            .into_iter()
            .flat_map(|p| {
                let lower = p.to_lowercase();
                let mut out = Vec::new();
                let mut start = 0;
                // byte offsets in `lower` match `p` only for same-length lowercase; fall back to
                // a char-index walk to stay correct for Cyrillic/Latin mixes.
                let pc: Vec<char> = p.chars().collect();
                let lc: Vec<char> = lower.chars().collect();
                let sc: Vec<char> = sep.chars().collect();
                if pc.len() != lc.len() {
                    return vec![p];
                }
                let mut i = 0;
                while i + sc.len() <= lc.len() {
                    if lc[i..i + sc.len()] == sc[..] {
                        out.push(pc[start..i].iter().collect::<String>());
                        i += sc.len();
                        start = i;
                    } else {
                        i += 1;
                    }
                }
                out.push(pc[start..].iter().collect::<String>());
                out
            })
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    let mut seen = Vec::<String>::new();
    for p in parts {
        if !seen.iter().any(|s| s.eq_ignore_ascii_case(&p)) {
            seen.push(p);
        }
    }
    seen
}

pub struct TagIdentify;

/// FR-IMP-007: longer than 25 min, or longer than 15 min when unidentified.
const LONG_MS: u64 = 25 * 60 * 1000;
const LONG_UNKNOWN_MS: u64 = 15 * 60 * 1000;

fn stem(file_name: &str) -> String {
    file_name
        .rsplit_once('.')
        .map_or(file_name, |(s, _)| s)
        .trim()
        .to_string()
}

impl Identify for TagIdentify {
    fn identify(
        &self,
        t: &Tags,
        file_name: &str,
        _folder: Option<&str>,
        info: &MediaInfo,
    ) -> IdentityDecision {
        let artists = t.artist.as_deref().map(split_artists).unwrap_or_default();
        let known = t.title.is_some() && !artists.is_empty();
        let unknown =
            if info.duration_ms > LONG_MS || (!known && info.duration_ms > LONG_UNKNOWN_MS) {
                Some(UnknownReason::PossibleNonTrack)
            } else if !known {
                Some(UnknownReason::NoMetadata)
            } else {
                None
            };
        IdentityDecision {
            title: t.title.clone().unwrap_or_else(|| stem(file_name)),
            artists,
            album: t.album.clone(),
            album_artist: t.album_artist.clone(),
            track_no: t.track_no,
            disc_no: t.disc_no,
            disc_total: t.disc_total,
            year: t.year,
            genre: t.genre.clone(),
            confidence: if known { 0.8 } else { 0.1 },
            unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_navidrome() {
        assert_eq!(split_artists("Би-2 feat. Земфира"), ["Би-2", "Земфира"]);
        assert_eq!(split_artists("Simon & Garfunkel"), ["Simon", "Garfunkel"]);
        assert_eq!(split_artists("A, B & C"), ["A", "B", "C"]);
        assert_eq!(split_artists("Земфира"), ["Земфира"]);
        assert_eq!(split_artists("Би-2 FEAT. Земфира"), ["Би-2", "Земфира"]);
        assert_eq!(split_artists("Ночные Снайперы"), ["Ночные Снайперы"]);
    }

    fn info(ms: u64) -> MediaInfo {
        MediaInfo {
            container: "mp3".into(),
            codec: "mp3".into(),
            lossless: false,
            sample_rate: None,
            channels: None,
            duration_ms: ms,
        }
    }

    #[test]
    fn decisions() {
        let tagged = Tags {
            title: Some("Варвара".into()),
            artist: Some("Би-2".into()),
            album: Some("Иномарки".into()),
            ..Tags::default()
        };
        let d = TagIdentify.identify(&tagged, "01.mp3", None, &info(200_000));
        assert_eq!(
            (d.unknown, d.artists.as_slice(), d.album.as_deref()),
            (None, &["Би-2".to_string()][..], Some("Иномарки"))
        );
        let bare = TagIdentify.identify(&Tags::default(), "Track 01.mp3", None, &info(200_000));
        assert_eq!(
            (bare.unknown, bare.title.as_str()),
            (Some(UnknownReason::NoMetadata), "Track 01")
        );
        let mix = TagIdentify.identify(&tagged, "mix.mp3", None, &info(3 * 3600 * 1000));
        assert_eq!(mix.unknown, Some(UnknownReason::PossibleNonTrack));
        let long_unknown =
            TagIdentify.identify(&Tags::default(), "a.mp3", None, &info(16 * 60 * 1000));
        assert_eq!(long_unknown.unknown, Some(UnknownReason::PossibleNonTrack));
    }
}
