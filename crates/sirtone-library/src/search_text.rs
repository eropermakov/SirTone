//! Query forms for search II (T3.15, FR-SRCH-003; `05` §9 step 1, `01b` F-17): the keyboard
//! layout fixed (ghbdtn → привет, нуыеуквфн → yesterday), transliteration both ways (zemfira →
//! земфира, би-2 → bi-2), Snowball stems (Russian/English, `rust-stemmers`) and stop words.
//! Every form is searched; the original weighs most.

use rust_stemmers::{Algorithm, Stemmer};
use sirtone_core::text::norm;

/// ЙЦУКЕН ↔ QWERTY, key for key (the lower row of each script).
const EN_KEYS: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,.`";
const RU_KEYS: &str = "йцукенгшщзхъфывапролджэячсмитьбюё";

fn is_cyr(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

/// The same keys typed in the other layout; `None` when nothing changes.
pub fn layout_swap(raw: &str) -> Option<String> {
    let lower = raw.to_lowercase();
    let (cyr, lat) = lower.chars().fold((0, 0), |(c, l), ch| {
        (
            c + usize::from(is_cyr(ch)),
            l + usize::from(ch.is_ascii_alphabetic()),
        )
    });
    let (from, to): (Vec<char>, Vec<char>) = if lat > cyr {
        (EN_KEYS.chars().collect(), RU_KEYS.chars().collect())
    } else {
        (RU_KEYS.chars().collect(), EN_KEYS.chars().collect())
    };
    let out: String = lower
        .chars()
        .map(|c| from.iter().position(|&f| f == c).map_or(c, |i| to[i]))
        .collect();
    (out != lower).then_some(out)
}

const LAT_MULTI: &[(&str, &str)] = &[
    ("shch", "щ"),
    ("sch", "щ"),
    ("zh", "ж"),
    ("kh", "х"),
    ("ts", "ц"),
    ("ch", "ч"),
    ("sh", "ш"),
    ("yu", "ю"),
    ("ya", "я"),
    ("yo", "ё"),
    ("ia", "ия"),
    ("iy", "ий"),
    ("yi", "ый"),
];

fn lat_single(c: char) -> Option<&'static str> {
    Some(match c {
        'a' => "а",
        'b' => "б",
        'v' | 'w' => "в",
        'g' => "г",
        'd' => "д",
        'e' => "е",
        'z' => "з",
        'i' => "и",
        'j' => "й",
        'k' | 'q' => "к",
        'l' => "л",
        'm' => "м",
        'n' => "н",
        'o' => "о",
        'p' => "п",
        'r' => "р",
        's' => "с",
        't' => "т",
        'u' => "у",
        'f' => "ф",
        'h' => "х",
        'c' => "ц",
        'x' => "кс",
        'y' => "ы",
        _ => return None,
    })
}

/// Latin → Cyrillic (simple practical transliteration: «zemfira» → «земфира», «bi 2» → «би 2»).
pub fn lat_to_cyr(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() * 2);
    let mut i = 0;
    'outer: while i < chars.len() {
        let rest: String = chars[i..chars.len().min(i + 4)].iter().collect();
        for (k, v) in LAT_MULTI {
            if rest.starts_with(k) {
                out.push_str(v);
                i += k.chars().count();
                continue 'outer;
            }
        }
        let c = chars[i];
        // «y» after a vowel or at a word end is «й» (Chaif, Splin's «moy»).
        if c == 'y' {
            let prev_vowel = i > 0 && "aeiou".contains(chars[i - 1]);
            let end = chars.get(i + 1).is_none_or(|n| !n.is_ascii_alphabetic());
            out.push_str(if prev_vowel || end { "й" } else { "ы" });
        } else {
            match lat_single(c) {
                Some(v) => out.push_str(v),
                None => out.push(c),
            }
        }
        i += 1;
    }
    out
}

/// Cyrillic → Latin (the common practical scheme: «би 2» → «bi 2», «щ» → «shch»).
pub fn cyr_to_lat(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let v = match c {
            'а' => "a",
            'б' => "b",
            'в' => "v",
            'г' => "g",
            'д' => "d",
            'е' | 'э' => "e",
            'ё' => "yo",
            'ж' => "zh",
            'з' => "z",
            'и' => "i",
            'й' | 'ы' => "y",
            'к' => "k",
            'л' => "l",
            'м' => "m",
            'н' => "n",
            'о' => "o",
            'п' => "p",
            'р' => "r",
            'с' => "s",
            'т' => "t",
            'у' => "u",
            'ф' => "f",
            'х' => "kh",
            'ц' => "ts",
            'ч' => "ch",
            'ш' => "sh",
            'щ' => "shch",
            'ъ' | 'ь' => "",
            'ю' => "yu",
            'я' => "ya",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(v);
    }
    out
}

const STOP: &[&str] = &[
    "и", "в", "во", "на", "с", "со", "к", "ко", "по", "за", "из", "у", "о", "об", "от", "для",
    "the", "a", "an", "of", "to", "in", "on", "and",
];

/// A query form and how much it weighs against the original.
#[derive(Debug, Clone, PartialEq)]
pub struct Form {
    /// `norm()` text.
    pub text: String,
    pub weight: f64,
}

/// The original, the layout-fixed and the transliterated forms (deduplicated).
pub fn forms(raw: &str) -> Vec<Form> {
    let Some(base) = norm(raw) else {
        return Vec::new();
    };
    let mut out = vec![Form {
        text: base.clone(),
        weight: 1.0,
    }];
    let mut push = |t: Option<String>, weight: f64| {
        if let Some(t) = t.and_then(|t| norm(&t))
            && !out.iter().any(|f| f.text == t)
        {
            out.push(Form { text: t, weight });
        }
    };
    push(layout_swap(raw), 0.9);
    let cyr = base.chars().filter(|&c| is_cyr(c)).count();
    let lat = base.chars().filter(char::is_ascii_alphabetic).count();
    if lat > cyr {
        push(Some(lat_to_cyr(&base)), 0.85);
    } else if cyr > 0 {
        push(Some(cyr_to_lat(&base)), 0.85);
    }
    out
}

/// FTS5 tokens for a form: each word as a prefix; with `expand` also its Snowball stem as a
/// prefix (words of four letters or more) and its «ё» forms; stop words dropped when other
/// words remain.
pub fn fts_query(text: &str, expand: bool) -> Option<String> {
    let ru = Stemmer::create(Algorithm::Russian);
    let en = Stemmer::create(Algorithm::English);
    let words: Vec<&str> = text.split_whitespace().collect();
    let content: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| !STOP.contains(w))
        .collect();
    let words = if content.is_empty() { words } else { content };
    let parts: Vec<String> = words
        .iter()
        .map(|w| {
            let w = w.replace('"', "");
            let stem = if expand && w.chars().count() >= 4 {
                let s = if w.chars().any(is_cyr) {
                    ru.stem(&w)
                } else {
                    en.stem(&w)
                };
                (s.chars().count() >= 3 && s != w).then(|| s.into_owned())
            } else {
                None
            };
            // The index keeps «ё» as typed: «все» also looks for «всё».
            let mut alts = vec![format!("\"{w}\"*")];
            if let Some(s) = stem {
                alts.push(format!("\"{s}\"*"));
            }
            if expand {
                for y in yo_forms(&w) {
                    alts.push(format!("\"{y}\"*"));
                }
            }
            if alts.len() == 1 {
                alts.remove(0)
            } else {
                format!("({})", alts.join(" OR "))
            }
        })
        .filter(|p| p != "\"\"*")
        .collect();
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

/// A Cyrillic word with «е» also as «ё» (each «е» at a time, at most three forms): `norm`
/// folds «ё» in queries, the FTS index does not.
fn yo_forms(w: &str) -> Vec<String> {
    if !w.chars().any(is_cyr) {
        return Vec::new();
    }
    let chars: Vec<char> = w.chars().collect();
    chars
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == 'е')
        .take(3)
        .map(|(i, _)| {
            chars
                .iter()
                .enumerate()
                .map(|(j, c)| if j == i { 'ё' } else { *c })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_both_ways() {
        assert_eq!(layout_swap("ghbdtn").as_deref(), Some("привет"));
        assert_eq!(layout_swap("нуыеуквфн").as_deref(), Some("yesterday"));
        assert_eq!(layout_swap("Rbyj").as_deref(), Some("кино"));
        assert_eq!(layout_swap("123"), None);
    }

    #[test]
    fn transliteration() {
        assert_eq!(lat_to_cyr("zemfira"), "земфира");
        assert_eq!(lat_to_cyr("bi 2"), "би 2");
        assert_eq!(lat_to_cyr("mumiy troll"), "мумий тролл");
        assert_eq!(lat_to_cyr("chaif"), "чаиф");
        assert_eq!(lat_to_cyr("aria"), "ария");
        assert_eq!(cyr_to_lat("би 2"), "bi 2");
        assert_eq!(cyr_to_lat("щука"), "shchuka");
    }

    #[test]
    fn forms_and_stems() {
        let f = forms("Zemfira");
        assert_eq!(f[0].text, "zemfira");
        assert!(f.iter().any(|x| x.text == "земфира"));
        assert!(fts_query("все", true).unwrap().contains("\"всё\"*"));
        let q = fts_query("песни кино", true).unwrap();
        assert!(q.contains("\"песн\"*"), "{q}");
        assert_eq!(fts_query("и", true).as_deref(), Some("\"и\"*"));
        assert!(
            fts_query("что такое и осень", true)
                .unwrap()
                .matches('*')
                .count()
                >= 3
        );
    }
}
