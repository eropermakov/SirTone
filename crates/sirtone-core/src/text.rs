//! Text keys shared by library, recognition and search (docs/05 §1).

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}' | '\u{0500}'..='\u{052F}')
}

fn is_latin(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '\u{00C0}'..='\u{024F}' | '\u{1E00}'..='\u{1EFF}')
}

/// `norm()` from docs/05 §1.1: NFKC → casefold → `ё→е` → strip Latin diacritics (Cyrillic `й`
/// kept) → `&` as «and»/«и» → apostrophes unified → dashes, dots, brackets, slashes → space →
/// collapse spaces. Articles are kept. Returns `None` when nothing is left.
pub fn norm(s: &str) -> Option<String> {
    // 1. NFKC + full lowercase (casefold differs only for a few letters; ß handled explicitly).
    let lowered: String = s
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .replace('ß', "ss");
    let cyrillic_context = lowered.chars().filter(|c| is_cyrillic(*c)).count()
        > lowered.chars().filter(|c| is_latin(*c)).count();

    let mut out = String::with_capacity(lowered.len());
    for c in lowered.chars() {
        match c {
            'ё' => out.push('е'),
            '&' => out.push_str(if cyrillic_context { " и " } else { " and " }),
            '’' | '‘' | '‛' | 'ʼ' | '`' => out.push('\''),
            '-' | '‐' | '‑' | '‒' | '–' | '—' | '―' | '_' | '.' | '(' | ')' | '[' | ']' | '{'
            | '}' | '/' | '\\' => out.push(' '),
            c if is_latin(c) && !c.is_ascii() => {
                // 2. Latin only: NFD, drop combining marks.
                out.extend(c.to_string().nfd().filter(|m| !is_combining_mark(*m)));
            }
            c => out.push(c),
        }
    }
    // 4. Collapse whitespace.
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then_some(collapsed)
}

#[cfg(test)]
mod tests {
    use super::norm;

    #[test]
    fn spec_examples() {
        // docs/05 §1.1
        assert_eq!(norm("Би-2").as_deref(), Some("би 2"));
        assert_eq!(norm("Би 2").as_deref(), Some("би 2"));
        assert_eq!(norm("Ёлка").as_deref(), Some("елка"));
        assert_eq!(norm("AC/DC").as_deref(), Some("ac dc"));
        assert_eq!(norm("Beyoncé").as_deref(), Some("beyonce"));
    }

    #[test]
    fn keeps_cyrillic_short_i_and_articles() {
        assert_eq!(norm("Мой Бойфренд").as_deref(), Some("мой бойфренд"));
        assert_eq!(norm("The Beatles").as_deref(), Some("the beatles"));
    }

    #[test]
    fn ampersand_follows_script_and_apostrophes_unify() {
        assert_eq!(
            norm("Simon & Garfunkel").as_deref(),
            Some("simon and garfunkel")
        );
        assert_eq!(norm("Чай & Кофе").as_deref(), Some("чай и кофе"));
        assert_eq!(norm("Don’t Stop").as_deref(), Some("don't stop"));
    }

    #[test]
    fn fullwidth_and_empty() {
        assert_eq!(norm("ＡＢＣ").as_deref(), Some("abc"));
        assert_eq!(norm(" — ( ) "), None);
        assert_eq!(norm("Straße").as_deref(), Some("strasse"));
    }
}
