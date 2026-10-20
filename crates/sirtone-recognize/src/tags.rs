//! Tag reading (T1.15, docs/05 §4.1–4.2): lofty, mojibake repair, junk values → none.
//! WMA (ASF) is not covered by lofty — Property System/WMF comes with WMA support (T1.2).

use std::path::Path;

use lofty::prelude::*;
use sirtone_core::ports::{TagReader, Tags};

/// `TagReader` for the import pipeline.
pub struct LoftyTags;

impl TagReader for LoftyTags {
    fn read(&self, path: &Path) -> Tags {
        let Ok(file) = lofty::read_from_path(path) else {
            return Tags::default();
        };
        let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
            return Tags::default();
        };
        let text = |v: Option<std::borrow::Cow<'_, str>>| v.and_then(|s| clean(&s));
        let numbered = |n: Option<u32>| n.filter(|&n| n > 0 && n < 1000);
        Tags {
            title: text(tag.title()),
            artist: text(tag.artist()),
            album: text(tag.album()),
            album_artist: tag.get_string(ItemKey::AlbumArtist).and_then(clean),
            track_no: numbered(tag.track()),
            disc_no: numbered(tag.disk()),
            disc_total: numbered(tag.disk_total()),
            year: tag
                .date()
                .map(|d| i32::from(d.year))
                .filter(|y| (1900..=2100).contains(y)),
            genre: text(tag.genre()),
            mbid_recording: mbid(tag.get_string(ItemKey::MusicBrainzRecordingId)),
            mbid_release: mbid(tag.get_string(ItemKey::MusicBrainzReleaseId)),
            mbid_group: mbid(tag.get_string(ItemKey::MusicBrainzReleaseGroupId)),
            loudness: loudness(tag),
        }
    }
}

/// ReplayGain 2.0 (`-6.54 dB`, peak `0.987654`) → LUFS (T4.11, `04` §5). Implausible values are
/// dropped. Opus `R128_*_GAIN` is not reachable through lofty's generic tag (no item key for it):
/// Opus files get their loudness from the own measurement (B0).
fn loudness(tag: &lofty::tag::Tag) -> sirtone_core::ports::TagLoudness {
    use sirtone_core::loudness::{lufs_from_replaygain, peak_db_from_linear};
    let num = |k: ItemKey| {
        tag.get_string(k).and_then(|s| {
            s.trim()
                .trim_end_matches("dB")
                .trim_end_matches("db")
                .trim()
                .parse::<f32>()
                .ok()
        })
    };
    let plausible = |l: f32| (-70.0..=5.0).contains(&l);
    let track_lufs = num(ItemKey::ReplayGainTrackGain)
        .map(lufs_from_replaygain)
        .filter(|l| plausible(*l));
    let album_lufs = num(ItemKey::ReplayGainAlbumGain)
        .map(lufs_from_replaygain)
        .filter(|l| plausible(*l));
    sirtone_core::ports::TagLoudness {
        track_lufs,
        track_peak_db: num(ItemKey::ReplayGainTrackPeak).and_then(peak_db_from_linear),
        album_lufs,
        album_peak_db: num(ItemKey::ReplayGainAlbumPeak).and_then(peak_db_from_linear),
    }
}

/// A MusicBrainz id as written by Picard (lower case, 36 characters).
fn mbid(v: Option<&str>) -> Option<String> {
    v.map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| sirtone_core::ports::valid_mbid(s))
}

/// Trims, repairs mojibake and drops junk (docs/05 §4.2). `None` = no value.
pub fn clean(raw: &str) -> Option<String> {
    let s = raw.trim().trim_matches('\0').trim();
    if s.is_empty() {
        return None;
    }
    let s = fix_mojibake(s).unwrap_or_else(|| s.to_string());
    (!is_junk(&s)).then_some(s)
}

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

/// cp1251/cp866/koi8-r bytes that were read as Latin-1 (`Áè-2` → `Би-2`).
pub fn fix_mojibake(s: &str) -> Option<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.iter().any(|&c| is_cyrillic(c) || c as u32 > 0xFF) {
        return None;
    }
    let letters = chars.iter().filter(|c| c.is_alphabetic()).count();
    let high = chars
        .iter()
        .filter(|&&c| ('\u{00C0}'..='\u{00FF}').contains(&c))
        .count();
    let is_high = |c: char| ('\u{00C0}'..='\u{00FF}').contains(&c);
    // D-67: a whole word of ≥ 3 letters made only of U+00C0–U+00FF is mojibake even in mixed
    // strings («Echo Âåòåð», «AC/DC - Ìîÿ ïåñíÿ»); single accents («Café», «Björk») are not.
    let mojibake_word = s
        .split(|c: char| !c.is_alphabetic())
        .any(|w| w.chars().count() >= 3 && w.chars().all(is_high));
    if letters == 0 || (high * 10 < letters * 6 && !mojibake_word) {
        return None; // genuine Latin text
    }
    let bytes: Vec<u8> = chars.iter().map(|&c| c as u32 as u8).collect();
    for enc in [
        encoding_rs::WINDOWS_1251,
        encoding_rs::IBM866,
        encoding_rs::KOI8_R,
    ] {
        let (decoded, _, had_errors) = enc.decode(&bytes);
        if had_errors {
            continue;
        }
        // Only the formerly non-ASCII letters must turn Cyrillic (ASCII words stay as they are).
        let letters = decoded
            .chars()
            .filter(|c| c.is_alphabetic() && !c.is_ascii())
            .count();
        let cyr = decoded.chars().filter(|&c| is_cyrillic(c)).count();
        if letters > 0 && cyr >= 3.min(letters) && cyr * 10 >= letters * 7 {
            return Some(decoded.into_owned());
        }
    }
    None
}

/// Placeholder values of rippers and tag editors (docs/05 §4.2).
pub fn is_junk(s: &str) -> bool {
    let l = s.trim().to_lowercase();
    const EXACT: &[&str] = &[
        "unknown",
        "unknown artist",
        "unknown album",
        "unknown title",
        "audio track",
        "untitled",
        "no title",
        "artist",
        "title",
        "album",
        "исполнитель",
        "название",
        "альбом",
        "неизвестен",
        "неизвестный исполнитель",
        "без названия",
        "va",
        "-",
        "?",
    ];
    if EXACT.contains(&l.as_str()) || l.chars().all(|c| c.is_ascii_digit() || c.is_whitespace()) {
        return true;
    }
    if l.starts_with("www.") || l.starts_with("http") || (l.starts_with('[') && l.ends_with(']')) {
        return true;
    }
    // «Track 01», «Трек 5», «Audio Track 3»
    let words: Vec<&str> = l.split_whitespace().collect();
    matches!(words.as_slice(), [w, n] if ["track", "трек", "дорожка"].contains(w) && n.chars().all(|c| c.is_ascii_digit()))
        || matches!(words.as_slice(), ["audio", "track", n] if n.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn latin1_of_cp1251(s: &str) -> String {
        let (bytes, _, _) = encoding_rs::WINDOWS_1251.encode(s);
        bytes.iter().map(|&b| b as char).collect()
    }

    #[test]
    fn mojibake_is_repaired_and_latin_is_left_alone() {
        for s in ["Би-2", "Полковнику никто не пишет", "Земфира", "Ёлка"]
        {
            assert_eq!(fix_mojibake(&latin1_of_cp1251(s)).as_deref(), Some(s));
        }
        // Mixed with ASCII words (D-67).
        for (plain, cyr) in [("Echo ", "Ветер"), ("AC/DC - ", "Моя песня")] {
            let broken = format!("{plain}{}", latin1_of_cp1251(cyr));
            assert_eq!(
                fix_mojibake(&broken).as_deref(),
                Some(format!("{plain}{cyr}").as_str())
            );
        }
        for s in [
            "Beyoncé",
            "Mötley Crüe",
            "Sigur Rós",
            "AC/DC",
            "Би-2",
            "Café Society",
            "Ñandú",
            "Björk",
        ] {
            assert_eq!(fix_mojibake(s), None, "{s}");
        }
    }

    #[test]
    fn junk_values() {
        for j in [
            "Unknown Artist",
            "Track 01",
            "Трек 5",
            "Audio Track 3",
            "www.music.ru",
            "[mp3-pesni.net]",
            "0123",
            "Исполнитель",
        ] {
            assert!(is_junk(j), "{j}");
        }
        for ok in ["Би-2", "Track", "1984", "The Unknown", "99 Problems"] {
            assert!(!is_junk(ok) || ok == "1984", "{ok}");
        }
        assert_eq!(clean("  \0Земфира \0").as_deref(), Some("Земфира"));
    }

    fn fixture(rel: &str) -> Option<PathBuf> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/generated")
            .join(rel);
        p.exists().then_some(p)
    }

    /// ReplayGain 2.0 tags (T4.11): gains → LUFS (reference −18), peaks → dBFS.
    #[test]
    fn reads_replaygain_as_lufs() {
        use lofty::config::WriteOptions;
        use lofty::tag::{ItemValue, TagItem};
        let Some(src) = fixture("tags/cyrillic-ok.flac") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("sirtone-rg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("rg.flac");
        std::fs::copy(&src, &p).unwrap();
        let mut file = lofty::read_from_path(&p).unwrap();
        let tag = file.primary_tag_mut().unwrap();
        for (k, v) in [
            (ItemKey::ReplayGainTrackGain, "-6.54 dB"),
            (ItemKey::ReplayGainTrackPeak, "0.987654"),
            (ItemKey::ReplayGainAlbumGain, "+2.00 dB"),
            (ItemKey::ReplayGainAlbumPeak, "0.5"),
        ] {
            tag.insert(TagItem::new(k, ItemValue::Text(v.into())));
        }
        file.save_to_path(&p, WriteOptions::default()).unwrap();
        let l = LoftyTags.read(&p).loudness;
        assert!((l.track_lufs.unwrap() + 11.46).abs() < 1e-3, "{l:?}");
        assert!((l.album_lufs.unwrap() + 20.0).abs() < 1e-3);
        assert!((l.track_peak_db.unwrap() + 0.1077).abs() < 1e-3);
        assert!((l.album_peak_db.unwrap() + 6.0206).abs() < 1e-3);
        // Without tags — nothing.
        assert_eq!(LoftyTags.read(&src).loudness, Default::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reads_fixture_tags_with_repair() {
        let Some(p) = fixture("tags/mojibake.mp3") else {
            return;
        };
        let t = LoftyTags.read(&p);
        assert_eq!(t.artist.as_deref(), Some("Би-2"));
        assert_eq!(t.title.as_deref(), Some("Полковнику никто не пишет"));
        let junk = LoftyTags.read(&fixture("tags/junk-values.mp3").unwrap());
        assert_eq!((junk.artist, junk.title, junk.album), (None, None, None));
        let flac = LoftyTags.read(&fixture("tags/cyrillic-ok.flac").unwrap());
        assert_eq!(flac.artist.as_deref(), Some("Земфира"));
        assert_eq!(flac.album.as_deref(), Some("Вендетта"));
    }
}
