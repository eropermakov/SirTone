//! Smart search, offline (T3.16, FR-SRCH-004, `05` §9; `01b` F-17: the references do it with a
//! cloud LLM, we do it with a lexicon — D-88): «что-то грустное и медленное» → facets of the
//! same filters «Настроить волну» and smart playlists use (`assets/vibe`), so «Играть этот
//! вайб» and the list under «По смыслу» agree with the wave. A query counts as smart only when
//! every word that is not a stop word is understood — «Дорога» the song stays a song.

use serde::Deserialize;
use sirtone_core::text::norm;

use crate::error::LibraryError;

static RU: &str = include_str!("../../../assets/lexicon/ru.json");
static EN: &str = include_str!("../../../assets/lexicon/en.json");

#[derive(Debug, Clone, PartialEq)]
pub enum Facet {
    /// happy · upbeat · calm · sad · positive · negative
    Mood(String),
    /// energetic · aggressive · calm
    Character(String),
    /// ru · foreign · none
    Lang(String),
    /// workout · sleep · work · road · rest · party
    Activity(String),
    /// BPM range.
    Tempo(f32, f32),
}

#[derive(Deserialize)]
struct TermJson {
    #[serde(default)]
    stem: Option<String>,
    #[serde(default)]
    phrase: Option<String>,
    #[serde(default)]
    mood: Option<String>,
    #[serde(default)]
    character: Option<String>,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    activity: Option<String>,
    #[serde(default)]
    tempo: Option<[f32; 2]>,
}

#[derive(Deserialize)]
struct LexJson {
    schema: String,
    stop: Vec<String>,
    terms: Vec<TermJson>,
}

/// The lexicons of all languages, normalised.
pub struct Lexicon {
    stop: Vec<String>,
    stems: Vec<(String, Facet)>,
    phrases: Vec<(String, Facet)>,
}

const MOODS: [&str; 6] = ["happy", "upbeat", "calm", "sad", "positive", "negative"];
const CHARACTERS: [&str; 3] = ["energetic", "aggressive", "calm"];
const LANGS: [&str; 3] = ["ru", "foreign", "none"];
const ACTIVITIES: [&str; 6] = ["workout", "sleep", "work", "road", "rest", "party"];

fn bad(s: &str) -> LibraryError {
    LibraryError::Invalid(format!("lexicon: {s}"))
}

impl Lexicon {
    pub fn builtin() -> Result<Self, LibraryError> {
        Self::from_json(&[RU, EN])
    }

    pub fn from_json(files: &[&str]) -> Result<Self, LibraryError> {
        let mut lex = Self {
            stop: Vec::new(),
            stems: Vec::new(),
            phrases: Vec::new(),
        };
        for f in files {
            let j: LexJson = serde_json::from_str(f).map_err(|e| bad(&e.to_string()))?;
            if j.schema != "sirtone.lexicon/1" {
                return Err(bad("schema"));
            }
            lex.stop.extend(j.stop.iter().filter_map(|s| norm(s)));
            for t in j.terms {
                let facet = match (&t.mood, &t.character, &t.lang, &t.activity, t.tempo) {
                    (Some(m), None, None, None, None) if MOODS.contains(&m.as_str()) => {
                        Facet::Mood(m.clone())
                    }
                    (None, Some(c), None, None, None) if CHARACTERS.contains(&c.as_str()) => {
                        Facet::Character(c.clone())
                    }
                    (None, None, Some(l), None, None) if LANGS.contains(&l.as_str()) => {
                        Facet::Lang(l.clone())
                    }
                    (None, None, None, Some(a), None) if ACTIVITIES.contains(&a.as_str()) => {
                        Facet::Activity(a.clone())
                    }
                    (None, None, None, None, Some([lo, hi])) if lo < hi => Facet::Tempo(lo, hi),
                    _ => return Err(bad("term needs exactly one known facet")),
                };
                match (
                    t.stem.as_deref().and_then(norm),
                    t.phrase.as_deref().and_then(norm),
                ) {
                    (Some(s), None) => lex.stems.push((s, facet)),
                    (None, Some(p)) => lex.phrases.push((p, facet)),
                    _ => return Err(bad("term needs a stem or a phrase")),
                }
            }
        }
        Ok(lex)
    }

    /// The facets of a smart query; empty — not a smart query.
    pub fn parse(&self, query: &str) -> Vec<Facet> {
        let Some(mut q) = norm(query) else {
            return Vec::new();
        };
        let mut facets: Vec<Facet> = Vec::new();
        let push = |f: &Facet, facets: &mut Vec<Facet>| {
            if !facets.contains(f) {
                facets.push(f.clone());
            }
        };
        // Phrases first («без слов», «для сна»), then each word by its longest known start.
        for (p, f) in &self.phrases {
            let padded = format!(" {q} ");
            if padded.contains(&format!(" {p} ")) {
                push(f, &mut facets);
                q = padded.replace(&format!(" {p} "), " ").trim().to_string();
            }
        }
        for word in q.split_whitespace() {
            if self.stop.iter().any(|s| s == word) {
                continue;
            }
            let best = self
                .stems
                .iter()
                .filter(|(s, _)| word.starts_with(s.as_str()))
                .max_by_key(|(s, _)| s.chars().count());
            match best {
                Some((_, f)) => push(f, &mut facets),
                // A word we do not understand: an ordinary search.
                None => return Vec::new(),
            }
        }
        facets
    }
}

/// Smart playlist rules (`05` §8.1) for the facets: any value within a kind, all kinds
/// together; the character thresholds are those of `assets/vibe/moods.json`.
pub fn rules_json(facets: &[Facet], limit: u32) -> String {
    use serde_json::{Value, json};
    let mut groups: Vec<Value> = Vec::new();
    let any = |v: Vec<Value>| {
        if v.len() == 1 {
            v.into_iter().next().unwrap_or(Value::Null)
        } else {
            json!({ "any": v })
        }
    };
    let pick = |f: &dyn Fn(&Facet) -> Option<Value>| -> Vec<Value> {
        facets.iter().filter_map(f).collect()
    };
    let moods = pick(&|f| match f {
        Facet::Mood(m) => Some(json!({ "is": { "mood": m } })),
        _ => None,
    });
    let chars = pick(&|f| match f {
        Facet::Character(c) => Some(match c.as_str() {
            "energetic" => json!({ "gt": { "energy": 0.65 } }),
            "aggressive" => json!({ "gt": { "aggression": 0.6 } }),
            _ => {
                json!({ "all": [ { "lt": { "energy": 0.35 } }, { "lt": { "aggression": 0.3 } } ] })
            }
        }),
        _ => None,
    });
    let langs = pick(&|f| match f {
        Facet::Lang(l) => Some(json!({ "is": { "lang": l } })),
        _ => None,
    });
    let acts = pick(&|f| match f {
        Facet::Activity(a) => Some(json!({ "is": { "activity": a } })),
        _ => None,
    });
    let tempo = pick(&|f| match f {
        Facet::Tempo(lo, hi) => Some(json!({ "inTheRange": { "bpm": [lo, hi] } })),
        _ => None,
    });
    for g in [moods, chars, langs, acts, tempo] {
        if !g.is_empty() {
            groups.push(any(g));
        }
    }
    json!({ "all": groups, "sort": "-play_count", "limit": limit, "refresh": "live" }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex() -> Lexicon {
        Lexicon::builtin().expect("lexicon")
    }

    #[test]
    fn sad_and_slow() {
        let f = lex().parse("Что-то грустное и медленное");
        assert_eq!(f, vec![Facet::Mood("sad".into()), Facet::Tempo(0.0, 90.0)]);
        let rules = rules_json(&f, 50);
        crate::smart::Rules::parse(&rules).expect("valid rules");
    }

    #[test]
    fn phrases_longest_stems_and_english() {
        let l = lex();
        assert_eq!(
            l.parse("спокойная музыка без слов для сна"),
            vec![
                Facet::Lang("none".into()),
                Facet::Activity("sleep".into()),
                Facet::Character("calm".into()),
            ]
        );
        // «workout» beats «work».
        assert_eq!(
            l.parse("songs for a workout"),
            vec![Facet::Activity("workout".into())]
        );
        assert_eq!(l.parse("something sad"), vec![Facet::Mood("sad".into())]);
        for f in [
            l.parse("тяжёлое энергичное"),
            l.parse("быстрое русское для тренировки"),
            l.parse("весёлое или бодрое"),
        ] {
            assert!(!f.is_empty());
            crate::smart::Rules::parse(&rules_json(&f, 50)).expect("valid rules");
        }
    }

    #[test]
    fn ordinary_queries_stay_ordinary() {
        let l = lex();
        for q in [
            "Кино группа крови",
            "Земфира",
            "грустное кино",
            "",
            "the beatles",
        ] {
            assert!(l.parse(q).is_empty(), "{q}");
        }
    }
}
