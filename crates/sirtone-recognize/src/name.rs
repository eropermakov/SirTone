//! File and folder name parsing (T1.16, docs/05 §4.3). Separator is only «space-dash-space», so
//! «Би-2», «Jay-Z», «AC-DC» stay whole. Junk in brackets goes, versions (Remix, Live) stay.

use crate::tags::is_junk;

/// What a file name says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NameGuess {
    pub track: Option<u32>,
    pub artist: Option<String>,
    pub title: Option<String>,
    /// Pattern confidence: 0.9 / 0.8 / 0.5 / 0.3 (docs/05 §4.3).
    pub confidence: f64,
}

/// Bracketed or trailing junk of rips and downloads (docs/05 §4.3).
const JUNK_IN_BRACKETS: &[&str] = &[
    "official video",
    "official music video",
    "official audio",
    "lyrics",
    "lyric video",
    "hd",
    "hq",
    "audio",
    "music video",
    "video",
    "clip",
    "клип",
    "официальный клип",
    "320kbps",
    "320 kbps",
    "256kbps",
    "192kbps",
    "128kbps",
    "mp3",
    "flac",
    "free download",
    "visualizer",
];

fn is_bracket_junk(inner: &str) -> bool {
    let l = inner.trim().to_lowercase();
    l.starts_with("www.")
        || l.contains(".com")
        || l.contains(".ru")
        || l.contains(".net")
        || JUNK_IN_BRACKETS.contains(&l.as_str())
}

/// Removes `[www.site.ru]`, `(Official Video)`, `[320kbps]`, ` - Topic`; keeps `(Remix)`, `(Live)`.
fn strip_junk(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let close = match c {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            _ => None,
        };
        if let Some(close) = close
            && let Some(len) = chars[i + 1..].iter().position(|&x| x == close)
        {
            let inner: String = chars[i + 1..i + 1 + len].iter().collect();
            if is_bracket_junk(&inner) {
                i += len + 2;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    let mut t = out.split_whitespace().collect::<Vec<_>>().join(" ");
    for suffix in [" - Topic", " - topic"] {
        if let Some(stripped) = t.strip_suffix(suffix) {
            t = stripped.to_string();
        }
    }
    t.trim_matches(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .to_string()
}

fn is_dash(c: char) -> bool {
    matches!(c, '-' | '–' | '—' | '−')
}

/// Splits on «space-dash-space»; returns the parts in order.
fn split_dash(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut parts = Vec::new();
    let mut start = 0;
    let mut i = 1;
    while i + 1 < chars.len() {
        if is_dash(chars[i]) && chars[i - 1].is_whitespace() && chars[i + 1].is_whitespace() {
            parts.push(
                chars[start..i - 1]
                    .iter()
                    .collect::<String>()
                    .trim()
                    .to_string(),
            );
            start = i + 2;
            i += 2;
            continue;
        }
        i += 1;
    }
    parts.push(
        chars[start.min(chars.len())..]
            .iter()
            .collect::<String>()
            .trim()
            .to_string(),
    );
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

/// Leading track number: `01 `, `1.`, `01)`, `01-`, `01_` (1–3 digits, docs/05 §4.3 patterns 1/3).
fn leading_track(s: &str) -> Option<(u32, String)> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 3 {
        return None;
    }
    let rest = &s[digits.len()..];
    let sep: String = rest
        .chars()
        .take_while(|c| matches!(c, '.' | ')' | '-' | '_' | ' ') || is_dash(*c))
        .collect();
    if sep.is_empty() {
        return None; // «1984», «99 Problems» stay titles
    }
    let n: u32 = digits.parse().ok()?;
    let rest = rest[sep.len()..].trim().to_string();
    // «99 Problems»: a bare number + single space + words is ambiguous. Accept it when the rest is
    // «artist - title» (pattern 1) or the number is zero-padded («07 Title»); else it is a title.
    if sep == " " && !digits.starts_with('0') && split_dash(&rest).len() < 2 {
        return None;
    }
    (!rest.is_empty() && n > 0).then_some((n, rest))
}

/// «10 Танцы» → (10, «Танцы»): a 1–2 digit number and a single space. Ambiguous on its own
/// («99 Problems»), so callers use it only inside an Artist/Album folder tree.
pub fn loose_track(title: &str) -> Option<(u32, String)> {
    let (num, rest) = title.split_once(' ')?;
    if num.is_empty() || num.len() > 2 || !num.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = num.parse().ok()?;
    let rest = rest.trim();
    (n > 0 && !rest.is_empty() && !is_junk(rest)).then(|| (n, rest.to_string()))
}

fn clean(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty() && !is_junk(t)).then(|| t.to_string())
}

/// docs/05 §4.3 patterns 1–4 on a file name (extension removed here). `known_artist` resolves
/// «A - B - C»: a library artist that matches a prefix wins, otherwise the left part is the artist.
pub fn parse_file_name(file_name: &str, known_artist: impl Fn(&str) -> bool) -> NameGuess {
    let stem = match file_name.rsplit_once('.') {
        Some((s, ext)) if ext.len() <= 5 && !ext.contains(' ') => s,
        _ => file_name,
    };
    let base = strip_junk(&stem.replace('_', " "));
    let (track, rest) = match leading_track(&base) {
        Some((n, rest)) => (Some(n), rest),
        None => (None, base.clone()),
    };
    let parts = split_dash(&rest);
    match (track, parts.as_slice()) {
        (_, [one]) => NameGuess {
            track,
            artist: None,
            title: clean(one),
            confidence: if track.is_some() { 0.5 } else { 0.3 },
        },
        (_, [first, rest @ ..]) if !rest.is_empty() => {
            // «A - B - C»: try the longest known-artist prefix.
            let mut split = 1;
            for k in (1..parts.len()).rev() {
                if known_artist(&parts[..k].join(" - ")) {
                    split = k;
                    break;
                }
            }
            let artist = if split == 1 {
                first.clone()
            } else {
                parts[..split].join(" - ")
            };
            let title = parts[split..].join(" - ");
            NameGuess {
                track,
                artist: clean(&artist),
                // After an artist a number is a title («ATL - 1000»), not a track number — from
                // three digits on (owner beta, T7.6).
                title: clean(&title).or_else(|| {
                    let t = title.trim();
                    (t.len() >= 3 && t.chars().all(|c| c.is_ascii_digit())).then(|| t.to_string())
                }),
                confidence: if track.is_some() { 0.9 } else { 0.8 },
            }
        }
        _ => NameGuess::default(),
    }
}

/// What a folder name says: `Альбом (2004)`, `2004 - Альбом`, `Исполнитель - Альбом (2004)`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FolderGuess {
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i32>,
}

fn year_of(s: &str) -> Option<i32> {
    let y: i32 = s.trim().parse().ok()?;
    (1900..=2100).contains(&y).then_some(y)
}

pub fn parse_folder(name: &str) -> FolderGuess {
    let mut s = strip_junk(name);
    let mut year = None;
    // trailing «(2004)» / «[2004]»
    for (open, close) in [('(', ')'), ('[', ']')] {
        if s.ends_with(close)
            && let Some(at) = s.rfind(open)
            && let Some(y) = year_of(&s[at + 1..s.len() - 1])
        {
            year = Some(y);
            s = s[..at].trim().to_string();
        }
    }
    let parts = split_dash(&s);
    match parts.as_slice() {
        [y, album] if year_of(y).is_some() => FolderGuess {
            artist: None,
            album: clean(album),
            year: year_of(y),
        },
        [artist, album] => FolderGuess {
            artist: clean(artist),
            album: clean(album),
            year,
        },
        _ => FolderGuess {
            artist: None,
            album: clean(&s),
            year,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> (Option<u32>, Option<String>, Option<String>) {
        let g = parse_file_name(name, |a| a == "Сплин - Гранатовый");
        (g.track, g.artist, g.title)
    }
    fn s(x: &str) -> Option<String> {
        Some(x.to_string())
    }

    #[test]
    fn spec_patterns_and_fixture_names() {
        assert_eq!(
            p("01 - Би-2 - Полковнику никто не пишет.mp3"),
            (Some(1), s("Би-2"), s("Полковнику никто не пишет"))
        );
        assert_eq!(
            p("Jay-Z - 99 Problems (Official Video).mp3"),
            (None, s("Jay-Z"), s("99 Problems"))
        );
        assert_eq!(
            p("AC-DC - Back In Black [320kbps].mp3"),
            (None, s("AC-DC"), s("Back In Black"))
        );
        assert_eq!(p("03. Варвара.flac"), (Some(3), None, s("Варвара")));
        assert_eq!(p("Track 01.mp3"), (None, None, None), "junk title");
        assert_eq!(
            p("Би-2 - Варвара (Remix).mp3"),
            (None, s("Би-2"), s("Варвара (Remix)"))
        );
        assert_eq!(
            p("Земфира – Искала [www.music.ru].mp3"),
            (None, s("Земфира"), s("Искала"))
        );
        assert_eq!(
            p("1984.mp3"),
            (None, None, None),
            "digits only = no value (docs/05 §4.2)"
        );
        assert_eq!(p("99 Problems.mp3"), (None, None, s("99 Problems")));
        assert_eq!(
            p("07_Кино_-_Группа_крови.mp3"),
            (Some(7), s("Кино"), s("Группа крови"))
        );
        assert_eq!(p("Radiohead - Creep - Topic.mp3").1, s("Radiohead"));
        assert_eq!(
            p("Сплин - Гранатовый - Романс.mp3"),
            (None, s("Сплин - Гранатовый"), s("Романс")),
            "known artist prefix"
        );
        assert_eq!(p("A - B - C.mp3"), (None, s("A"), s("B - C")));
        assert_eq!(p("Moby - Porcelain.MP3").1, s("Moby"));
    }

    type Parsed = (Option<u32>, Option<String>, Option<String>);

    /// T1.16 acceptance: ≥ 98 % of 500 generated names parse to the exact (track, artist, title).
    #[test]
    fn accuracy_on_500_names() {
        let artists = [
            "Би-2",
            "Jay-Z",
            "AC-DC",
            "Земфира",
            "Мумий Тролль",
            "Sigur Rós",
            "Ночные Снайперы",
            "t.A.T.u.",
            "Ленинград",
            "Daft Punk",
        ];
        let titles = [
            "Полковнику никто не пишет",
            "99 Problems",
            "Back In Black",
            "Искала",
            "Владивосток 2000",
            "Hoppípolla",
            "Тридцать первая весна",
            "Нас не догонят",
            "WWW",
            "Get Lucky",
            "Мой рок-н-ролл",
            "Наше лето",
        ];
        let versions = ["", " (Remix)", " (Live)", " (Acoustic)"];
        let junk = [
            "",
            " (Official Video)",
            " [320kbps]",
            " [www.muzofon.ru]",
            " (Lyrics)",
        ];
        let mut total = 0;
        let mut ok = 0;
        let mut failures = Vec::new();
        for i in 0..500usize {
            let a = artists[i % artists.len()];
            let t0 = titles[(i * 7) % titles.len()];
            let v = versions[(i / 3) % versions.len()];
            let j = junk[(i / 5) % junk.len()];
            let n = (i % 14 + 1) as u32;
            let title = format!("{t0}{v}");
            let (name, want): (String, Parsed) = match i % 8 {
                0 => (
                    format!("{n:02} - {a} - {title}{j}.mp3"),
                    (Some(n), s(a), Some(title.clone())),
                ),
                1 => (
                    format!("{a} - {title}{j}.mp3"),
                    (None, s(a), Some(title.clone())),
                ),
                2 => (
                    format!("{n:02}. {title}{j}.flac"),
                    (Some(n), None, Some(title.clone())),
                ),
                3 => (
                    format!("{a} – {title}{j}.mp3"),
                    (None, s(a), Some(title.clone())),
                ),
                4 => (
                    format!(
                        "{n:02}_{}_-_{}.mp3",
                        a.replace(' ', "_"),
                        title.replace(' ', "_")
                    ),
                    (Some(n), s(a), Some(title.clone())),
                ),
                5 => (
                    format!("{n:02} {a} — {title}{j}.ogg"),
                    (Some(n), s(a), Some(title.clone())),
                ),
                6 => (
                    format!("{n}. {a} - {title}.m4a"),
                    (Some(n), s(a), Some(title.clone())),
                ),
                _ => (
                    format!("{a} - {title} - Topic.mp3"),
                    (None, s(a), Some(title.clone())),
                ),
            };
            total += 1;
            let got = parse_file_name(&name, |_| false);
            if (got.track, got.artist.clone(), got.title.clone()) == want {
                ok += 1;
            } else if failures.len() < 10 {
                failures.push(format!("{name} → {:?}", (got.track, got.artist, got.title)));
            }
        }
        let acc = ok as f64 / total as f64;
        assert!(acc >= 0.98, "accuracy {acc:.3}: {failures:#?}");
    }

    #[test]
    fn loose_track_numbers() {
        assert_eq!(loose_track("10 Танцы"), Some((10, "Танцы".into())));
        assert_eq!(loose_track("1984"), None);
        assert_eq!(loose_track("100 Years"), None);
    }

    #[test]
    fn folders() {
        assert_eq!(
            parse_folder("Иномарки (2004)"),
            FolderGuess {
                artist: None,
                album: s("Иномарки"),
                year: Some(2004)
            }
        );
        assert_eq!(
            parse_folder("2004 - Иномарки"),
            FolderGuess {
                artist: None,
                album: s("Иномарки"),
                year: Some(2004)
            }
        );
        assert_eq!(
            parse_folder("Би-2 - Иномарки [2004]"),
            FolderGuess {
                artist: s("Би-2"),
                album: s("Иномарки"),
                year: Some(2004)
            }
        );
        assert_eq!(parse_folder("Сборка").album, s("Сборка"));
    }
}
