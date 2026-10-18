//! Library layout (T1.13, docs/05 §2): path templates and `sanitize_component`
//! (beets `replace` + Picard «Windows compatibility»).

use unicode_normalization::UnicodeNormalization;

/// Max characters of one path component (docs/05 §2.2).
pub const MAX_COMPONENT: usize = 120;
/// Max characters of a library-relative path (headroom under MAX_PATH 259 with the root).
pub const MAX_REL_PATH: usize = 240;

const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// One path component that is valid on NTFS/exFAT/FAT32, idempotent and never empty.
pub fn sanitize_component(s: &str) -> String {
    // 1. Separators, Windows-invalid characters and controls → `_`.
    let mut out: String = s
        .nfc()
        .map(|c| match c {
            '\\' | '/' | '<' | '>' | ':' | '"' | '?' | '*' | '|' => '_',
            c if (c as u32) < 0x20 || c == '\u{7f}' => '_',
            c => c,
        })
        .collect();
    // 2. Leading spaces removed; leading `.`/`-` → `_`; trailing dots and spaces removed.
    out = out.trim_start().to_string();
    if out.starts_with('.') || out.starts_with('-') {
        out.replace_range(0..1, "_");
    }
    // 4. Length limit by characters (graphemes are approximated by chars + no split of a
    //    trailing combining mark).
    if out.chars().count() > MAX_COMPONENT {
        out = out.chars().take(MAX_COMPONENT).collect();
        while out
            .chars()
            .last()
            .is_some_and(unicode_normalization::char::is_combining_mark)
        {
            out.pop();
        }
    }
    out = out.trim_end_matches(['.', ' ']).to_string();
    if out.is_empty() {
        return "_".into();
    }
    // 3. Reserved device names, also with an extension (`con.mp3`) → suffix `_`.
    let stem = out
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        let at = out.find('.').unwrap_or(out.len());
        out.insert(at, '_');
    }
    out
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Where a track goes inside the library (docs/05 §2.1). Components are sanitised; the caller
/// resolves case-insensitive collisions (` (2)`) against the disk and the database.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// `/`-separated, relative to the library root, NFC (docs/03 §7 `rel_path`).
    pub rel_path: String,
}

pub struct TrackPlace<'a> {
    pub artist: &'a str,
    pub album_artist: Option<&'a str>,
    pub album: Option<&'a str>,
    pub title: &'a str,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    /// More than one disc in the album → `1-01 - …`.
    pub multi_disc: bool,
    pub ext: &'a str,
}

const VARIOUS: &[&str] = &[
    "various artists",
    "various",
    "va",
    "сборник",
    "разные исполнители",
];

pub fn canonical(p: &TrackPlace<'_>) -> Placement {
    let ext = sanitize_component(&p.ext.to_ascii_lowercase());
    let title = sanitize_component(p.title);
    let rel = match p.album {
        Some(album)
            if p.album_artist
                .is_some_and(|a| VARIOUS.contains(&a.trim().to_lowercase().as_str())) =>
        {
            let no = p.track_no.map(|n| format!("{n:02} - ")).unwrap_or_default();
            format!(
                "Compilations/{}/{}{} - {}.{ext}",
                sanitize_component(album),
                no,
                sanitize_component(p.artist),
                title
            )
        }
        Some(album) => {
            let disc = match (p.multi_disc, p.disc_no) {
                (true, Some(d)) => format!("{d}-"),
                _ => String::new(),
            };
            let no = p
                .track_no
                .map(|n| format!("{disc}{n:02} - "))
                .unwrap_or_default();
            format!(
                "{}/{}/{}{}.{ext}",
                sanitize_component(p.album_artist.unwrap_or(p.artist)),
                sanitize_component(album),
                no,
                title
            )
        }
        None => format!("{}/Singles/{}.{ext}", sanitize_component(p.artist), title),
    };
    Placement {
        rel_path: fit(&rel),
    }
}

/// `_Unknown/{original file name}` (docs/05 §2.1).
pub fn unknown(original_file_name: &str) -> Placement {
    Placement {
        rel_path: fit(&format!(
            "_Unknown/{}",
            sanitize_component(original_file_name)
        )),
    }
}

/// Shortens components (the longest first, the extension never) until the whole relative path
/// fits MAX_REL_PATH (docs/05 §2.2: ≤ 240, headroom under MAX_PATH).
fn fit(rel: &str) -> String {
    let mut parts: Vec<String> = rel.split('/').map(str::to_owned).collect();
    let Some(last) = parts.pop() else {
        return rel.to_string();
    };
    let (stem, ext) = match last.rsplit_once('.') {
        Some((s, e)) => (s.to_owned(), format!(".{e}")),
        None => (last.clone(), String::new()),
    };
    parts.push(stem);
    let total = |ps: &[String]| {
        ps.iter().map(|p| p.chars().count()).sum::<usize>() + ps.len() - 1 + ext.chars().count()
    };
    while total(&parts) > MAX_REL_PATH {
        let Some((i, longest)) = parts
            .iter()
            .enumerate()
            .max_by_key(|(_, p)| p.chars().count())
        else {
            break;
        };
        let len = longest.chars().count();
        if len <= 8 {
            break; // cannot shrink further without losing meaning
        }
        let over = total(&parts) - MAX_REL_PATH;
        let keep = len.saturating_sub(over).max(8).min(len - 1);
        let clipped = clip(longest, keep).trim_end_matches(['.', ' ']).to_string();
        parts[i] = if clipped.is_empty() {
            "_".into()
        } else {
            clipped
        };
    }
    let stem = parts.pop().unwrap_or_default();
    parts.push(format!("{stem}{ext}"));
    parts.join("/")
}

/// Adds ` (2)`, ` (3)`… before the extension until `taken` says the name is free.
pub fn deduplicate(rel: &str, mut taken: impl FnMut(&str) -> bool) -> String {
    if !taken(rel) {
        return rel.to_string();
    }
    let (dir, file) = rel.rsplit_once('/').map_or(("", rel), |(d, f)| (d, f));
    let (stem, ext) = file.rsplit_once('.').unwrap_or((file, ""));
    for n in 2.. {
        let f = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        let candidate = if dir.is_empty() {
            f
        } else {
            format!("{dir}/{f}")
        };
        if !taken(&candidate) {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn spec_rules() {
        assert_eq!(sanitize_component("AC/DC"), "AC_DC");
        assert_eq!(sanitize_component("What?*<>|"), "What_____");
        assert_eq!(sanitize_component(".hidden"), "_hidden");
        assert_eq!(sanitize_component("-intro"), "_intro");
        assert_eq!(sanitize_component("  trailing dot.. "), "trailing dot");
        assert_eq!(sanitize_component("con"), "con_");
        assert_eq!(sanitize_component("CON.mp3"), "CON_.mp3");
        assert_eq!(sanitize_component("Lpt1"), "Lpt1_");
        assert_eq!(sanitize_component("Песня с эмодзи 🎵"), "Песня с эмодзи 🎵");
        assert_eq!(sanitize_component(""), "_");
        assert_eq!(sanitize_component(" . "), "_");
        assert_eq!(
            sanitize_component(&"я".repeat(300)).chars().count(),
            MAX_COMPONENT
        );
    }

    #[test]
    fn templates() {
        let base = TrackPlace {
            artist: "Би-2",
            album_artist: None,
            album: Some("Иномарки"),
            title: "Варвара",
            track_no: Some(3),
            disc_no: Some(1),
            multi_disc: false,
            ext: "FLAC",
        };
        assert_eq!(canonical(&base).rel_path, "Би-2/Иномарки/03 - Варвара.flac");
        let multi = TrackPlace {
            multi_disc: true,
            disc_no: Some(2),
            ..base
        };
        assert_eq!(
            canonical(&multi).rel_path,
            "Би-2/Иномарки/2-03 - Варвара.flac"
        );
        let single = TrackPlace {
            album: None,
            ..base
        };
        assert_eq!(canonical(&single).rel_path, "Би-2/Singles/Варвара.flac");
        let va = TrackPlace {
            album_artist: Some("Various Artists"),
            album: Some("Хиты 2004"),
            ..base
        };
        assert_eq!(
            canonical(&va).rel_path,
            "Compilations/Хиты 2004/03 - Би-2 - Варвара.flac"
        );
        let feat = TrackPlace {
            album_artist: Some("Би-2"),
            artist: "Би-2 feat. Земфира",
            ..base
        };
        assert!(canonical(&feat).rel_path.starts_with("Би-2/Иномарки/"));
        assert_eq!(unknown("Track 01.mp3").rel_path, "_Unknown/Track 01.mp3");
    }

    #[test]
    fn long_paths_are_fitted() {
        let long = "Очень длинное имя ".repeat(12);
        let p = canonical(&TrackPlace {
            artist: &long,
            album_artist: None,
            album: Some(&long),
            title: &long,
            track_no: Some(1),
            disc_no: None,
            multi_disc: false,
            ext: "mp3",
        });
        assert!(p.rel_path.chars().count() <= MAX_REL_PATH, "{}", p.rel_path);
        assert!(p.rel_path.ends_with(".mp3"));
    }

    #[test]
    fn collisions_get_numbered() {
        let taken = ["a/b.mp3", "a/b (2).mp3"];
        assert_eq!(
            deduplicate("a/b.mp3", |c| taken.contains(&c)),
            "a/b (3).mp3"
        );
        assert_eq!(deduplicate("a/c.mp3", |c| taken.contains(&c)), "a/c.mp3");
    }

    fn valid_windows_component(s: &str) -> bool {
        !s.is_empty()
            && s.chars().count() <= MAX_COMPONENT
            && !s
                .chars()
                .any(|c| "\\/<>:\"?*|".contains(c) || (c as u32) < 0x20)
            && !s.ends_with('.')
            && !s.ends_with(' ')
            && !s.starts_with(' ')
            && !RESERVED.contains(
                &s.split('.')
                    .next()
                    .unwrap_or("")
                    .trim_end()
                    .to_ascii_lowercase()
                    .as_str(),
            )
    }

    proptest! {
        #[test]
        fn sanitize_is_valid_idempotent_and_nonempty(s in "\\PC{0,200}") {
            let once = sanitize_component(&s);
            prop_assert!(valid_windows_component(&once), "{:?} -> {:?}", s, once);
            prop_assert_eq!(sanitize_component(&once), once);
        }
    }
}
