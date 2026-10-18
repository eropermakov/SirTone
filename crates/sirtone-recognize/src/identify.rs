//! Offline identity decision (T1.16–T1.17, docs/05 §4.3–4.5): tags (T), file name (F), folders and
//! folder consensus (P). Online rows of the decision table (AcoustID/MusicBrainz) come with Stage 3.
//! Principle: no confidence → «Неизвестные», never a wrong folder.

use sirtone_core::ports::{
    FolderHints, Identify, IdentityDecision, MediaInfo, Tags, UnknownReason,
};
use sirtone_core::text::norm;

use crate::name::{loose_track, parse_file_name, parse_folder};

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
                let pc: Vec<char> = p.chars().collect();
                let lc: Vec<char> = p.to_lowercase().chars().collect();
                let sc: Vec<char> = sep.chars().collect();
                if pc.len() != lc.len() {
                    return vec![p];
                }
                let mut out = Vec::new();
                let (mut start, mut i) = (0, 0);
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

/// 0…1 similarity of two names after `norm` (Damerau–Levenshtein, also on sorted tokens).
pub fn similarity(a: &str, b: &str) -> f64 {
    let (Some(a), Some(b)) = (norm(a), norm(b)) else {
        return 0.0;
    };
    let direct = strsim::normalized_damerau_levenshtein(&a, &b);
    let sorted = |s: &str| {
        let mut t: Vec<&str> = s.split_whitespace().collect();
        t.sort_unstable();
        t.join(" ")
    };
    direct.max(strsim::normalized_damerau_levenshtein(
        &sorted(&a),
        &sorted(&b),
    ))
}

/// Folder names that never mean an artist or album.
const GENERIC_FOLDERS: &[&str] = &[
    "music",
    "музыка",
    "my music",
    "моя музыка",
    "downloads",
    "загрузки",
    "desktop",
    "рабочий стол",
    "mp3",
    "flac",
    "audio",
    "аудио",
    "new folder",
    "новая папка",
    "сборка",
    "сборник",
    "разное",
    "misc",
    "various",
    "various artists",
    "va",
    "без тегов",
    "unknown",
    "temp",
    "tmp",
    "плеер",
    "phone",
    "телефон",
    "старые mp3",
    "music library",
];

fn meaningful(folder: &Option<String>) -> Option<String> {
    folder
        .as_ref()
        .filter(|f| {
            let n = norm(f).unwrap_or_default();
            !n.is_empty() && !GENERIC_FOLDERS.contains(&n.as_str())
        })
        .cloned()
}

pub struct TagIdentify;

/// FR-IMP-007: longer than 25 min, or longer than 15 min when unidentified.
const LONG_MS: u64 = 25 * 60 * 1000;
const LONG_UNKNOWN_MS: u64 = 15 * 60 * 1000;
/// «Agree» and «conflict» thresholds for T/F/P names (docs/05 §4.5: agree at sim ≥ 0.9).
const AGREE: f64 = 0.9;
const CONFLICT: f64 = 0.5;

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
        folders: &FolderHints,
        info: &MediaInfo,
    ) -> IdentityDecision {
        let mut f = parse_file_name(file_name, |_| false);
        let parent = folders
            .parent
            .as_deref()
            .map(parse_folder)
            .unwrap_or_default();
        let parent_ok = meaningful(&folders.parent);
        let grand = meaningful(&folders.grandparent);
        // Inside «Artist/Album/» a single-space «10 Title» is a track number.
        if f.track.is_none()
            && f.artist.is_none()
            && grand.is_some()
            && parent_ok.is_some()
            && let Some((n, rest)) = f.title.as_deref().and_then(loose_track)
        {
            f.track = Some(n);
            f.title = Some(rest);
        }
        // Artist named by the folders: «Би-2/Иномарки (2004)/…» or «Би-2 - Иномарки/…».
        let folder_artist = parent.artist.clone().or_else(|| grand.clone());
        let folder_album = parent_ok.as_ref().and(parent.album.clone());

        let tag_artists = t.artist.as_deref().map(split_artists).unwrap_or_default();
        let base = |title: String,
                    artists: Vec<String>,
                    confidence: f64,
                    unknown: Option<UnknownReason>| IdentityDecision {
            title,
            artists,
            album: None,
            album_artist: None,
            track_no: t.track_no.or(f.track),
            disc_no: t.disc_no,
            disc_total: t.disc_total,
            year: t.year.or(parent.year),
            genre: t.genre.clone(),
            confidence,
            unknown,
        };

        let mut d = if let (Some(title), false) = (t.title.clone(), tag_artists.is_empty()) {
            // Tags name the track. Row 4: a file name that names a different track → conflict.
            let conflict = match (&f.artist, &f.title) {
                (Some(fa), Some(ft)) => {
                    similarity(fa, &tag_artists[0]) < CONFLICT && similarity(ft, &title) < CONFLICT
                }
                _ => false,
            };
            let agrees = matches!((&f.artist, &f.title), (Some(fa), Some(ft))
                if similarity(fa, &tag_artists[0]) >= AGREE && similarity(ft, &title) >= AGREE);
            let full = t.album.is_some() && t.track_no.is_some();
            let mut d = base(
                title,
                tag_artists.clone(),
                if conflict {
                    0.4
                } else if agrees || full {
                    0.9
                } else {
                    0.8
                },
                conflict.then_some(UnknownReason::Conflict),
            );
            d.album = t.album.clone().or_else(|| folders.consensus_album.clone());
            d.album_artist = t
                .album_artist
                .clone()
                .or_else(|| folders.consensus_album_artist.clone());
            d
        } else {
            // Rows 3/9: no usable tags — the file name counts only when the folders agree.
            // The artist may be named by the nearest folder itself («Сплин/Сплин - Романс.mp3»).
            let named_by_folder = |fa: &str| {
                [folder_artist.as_deref(), parent_ok.as_deref()]
                    .into_iter()
                    .flatten()
                    .any(|pa| similarity(fa, pa) >= AGREE)
            };
            match (&f.artist, &f.title, &folder_artist) {
                (Some(fa), Some(ft), _) if named_by_folder(fa) => {
                    let mut d = base(ft.clone(), split_artists(fa), 0.85, None);
                    d.album = folder_album.clone().filter(|a| similarity(a, fa) < AGREE);
                    d
                }
                // «Исполнитель/Альбом/03 Название» with no tags: the name carries no artist, so
                // nothing agrees with the folders (row 3) — CD1/genre/year folders would put it in
                // a wrong place. It goes to «Неизвестные»; the folder guess is only a hint.
                _ => {
                    let title = t
                        .title
                        .clone()
                        .or(f.title.clone())
                        .unwrap_or_else(|| stem(file_name));
                    let artists = if tag_artists.is_empty() {
                        f.artist.as_deref().map(split_artists).unwrap_or_default()
                    } else {
                        tag_artists
                    };
                    base(title, artists, 0.1, Some(UnknownReason::NoMetadata))
                }
            }
        };
        // Row 10 / FR-IMP-007.
        if info.duration_ms > LONG_MS || (d.unknown.is_some() && info.duration_ms > LONG_UNKNOWN_MS)
        {
            d.unknown = Some(UnknownReason::PossibleNonTrack);
        }
        if d.unknown.is_some() {
            d.album = None;
            d.album_artist = None;
        }
        d
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
    fn hints(parent: &str, grand: &str) -> FolderHints {
        FolderHints {
            parent: Some(parent.into()),
            grandparent: Some(grand.into()),
            ..FolderHints::default()
        }
    }
    fn tags(artist: &str, title: &str) -> Tags {
        Tags {
            artist: Some(artist.into()),
            title: Some(title.into()),
            ..Tags::default()
        }
    }

    #[test]
    fn decision_table_offline_rows() {
        let none = FolderHints::default();
        // Row 1/2: tags.
        let d = TagIdentify.identify(&tags("Би-2", "Варвара"), "01.mp3", &none, &info(200_000));
        assert_eq!((d.unknown, d.artists[0].as_str()), (None, "Би-2"));
        // Row 4: tags vs file name conflict.
        let d = TagIdentify.identify(
            &tags("Би-2", "Варвара"),
            "Земфира - Искала.mp3",
            &none,
            &info(200_000),
        );
        assert_eq!(d.unknown, Some(UnknownReason::Conflict));
        // Same track, typo in the name: no conflict.
        let d = TagIdentify.identify(
            &tags("Би-2", "Варвара"),
            "Би 2 - Варвра.mp3",
            &none,
            &info(200_000),
        );
        assert_eq!(d.unknown, None);
        // Row 3: name alone → unknown; name + folder artist → placed.
        let bare = Tags::default();
        let d = TagIdentify.identify(
            &bare,
            "Сплин - Романс.mp3",
            &hints("Без тегов", "Флешка"),
            &info(200_000),
        );
        assert_eq!(
            (d.unknown, d.title.as_str()),
            (Some(UnknownReason::NoMetadata), "Романс")
        );
        let d = TagIdentify.identify(
            &bare,
            "Сплин - Романс.mp3",
            &hints("Сплин", "Музыка"),
            &info(200_000),
        );
        assert_eq!(
            (d.unknown, d.artists[0].as_str(), d.title.as_str()),
            (None, "Сплин", "Романс")
        );
        // Artist/Album (year)/NN Title without tags: only folders say who it is → unknown,
        // and the risky layouts (CD1, genre, year folders) with it.
        for (parent, grand) in [
            ("Гранатовый альбом (1998)", "Сплин"),
            ("CD1", "Гранатовый альбом"),
            ("Гранатовый альбом", "Rock"),
            ("Гранатовый альбом", "2010"),
        ] {
            let d = TagIdentify.identify(
                &bare,
                "03 Романс.mp3",
                &hints(parent, grand),
                &info(200_000),
            );
            assert_eq!(
                d.unknown,
                Some(UnknownReason::NoMetadata),
                "{grand}/{parent}"
            );
            assert_eq!(d.title, "Романс");
        }
        // Generic folders never become artists.
        let d = TagIdentify.identify(
            &bare,
            "03 Романс.mp3",
            &hints("Новая папка", "Music"),
            &info(200_000),
        );
        assert_eq!(d.unknown, Some(UnknownReason::NoMetadata));
        // Folder consensus fills a missing album.
        let h = FolderHints {
            consensus_album: Some("Иномарки".into()),
            ..FolderHints::default()
        };
        let d = TagIdentify.identify(&tags("Би-2", "Варвара"), "x.mp3", &h, &info(200_000));
        assert_eq!(d.album.as_deref(), Some("Иномарки"));
        // Row 10.
        let d = TagIdentify.identify(&tags("DJ", "Mix"), "mix.mp3", &none, &info(3 * 3600 * 1000));
        assert_eq!(d.unknown, Some(UnknownReason::PossibleNonTrack));
    }
}
