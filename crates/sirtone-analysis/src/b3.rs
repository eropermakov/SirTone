//! Analysis B3 (T3.9, `04` §10.2): the vibe from the AudioSet classes of B2 — energy, valence,
//! arousal, danceability, acousticness, vocal, aggression, the eight mood tags and the six genre
//! groups of the wave vector (`14` §4.1). Rules live in `assets/vibe/b3.json`, classes by name
//! (`assets/models/audioset_labels.csv`); every number is explainable from the class
//! probabilities. Temporary weights until calibration (D-85).

use std::collections::HashMap;

use serde::Deserialize;
use sirtone_core::analysis::Vibe;

use crate::b2::{B2, CLASSES};

static RULES: &str = include_str!("../../../assets/vibe/b3.json");
static LABELS: &str = include_str!("../../../assets/models/audioset_labels.csv");

#[derive(Debug, thiserror::Error)]
pub enum B3Error {
    #[error("b3 rules: {0}")]
    Json(#[from] serde_json::Error),
    #[error("b3 rules: unknown class «{0}»")]
    Class(String),
    #[error("b3 rules: {0}")]
    Invalid(&'static str),
}

#[derive(Deserialize)]
struct ScalarJson {
    pos: HashMap<String, f32>,
    neg: HashMap<String, f32>,
    a: f32,
    b: f32,
}

#[derive(Deserialize)]
struct GroupsJson {
    classes: Vec<HashMap<String, f32>>,
    #[serde(default)]
    floor: f32,
}

#[derive(Deserialize)]
struct BrightJson {
    lo_hz: f32,
    hi_hz: f32,
}

#[derive(Deserialize)]
struct ConfJson {
    two_windows: f32,
    one_window: f32,
}

#[derive(Deserialize)]
struct RulesJson {
    schema: String,
    scalars: HashMap<String, ScalarJson>,
    moods: GroupsJson,
    genres: GroupsJson,
    brightness: BrightJson,
    instrumental_below: f32,
    conf: ConfJson,
}

type Weights = Vec<(usize, f32)>;

struct Scalar {
    pos: Weights,
    neg: Weights,
    a: f32,
    b: f32,
}

impl Scalar {
    fn eval(&self, p: &[f32]) -> f32 {
        let sum = |w: &Weights| w.iter().map(|&(i, k)| p[i] * k).sum::<f32>();
        let (pos, neg) = (sum(&self.pos), sum(&self.neg));
        ((pos + self.a) / (pos + neg + self.a + self.b)).clamp(0.0, 1.0)
    }
}

/// The compiled rules.
pub struct Rules {
    /// energy, valence, arousal, danceability, acousticness, vocal, aggression.
    scalars: [Scalar; 7],
    moods: [Weights; 8],
    mood_floor: f32,
    genres: [Weights; 6],
    bright: (f32, f32),
    instrumental_below: f32,
    conf: (f32, f32),
}

/// AudioSet display names → class index.
fn labels() -> HashMap<&'static str, usize> {
    LABELS
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut it = l.splitn(3, ',');
            let i = it.next()?.parse().ok()?;
            it.next()?;
            Some((it.next()?.trim().trim_matches('"'), i))
        })
        .collect()
}

impl Rules {
    pub fn builtin() -> Result<Self, B3Error> {
        Self::from_json(RULES)
    }

    pub fn from_json(json: &str) -> Result<Self, B3Error> {
        let r: RulesJson = serde_json::from_str(json)?;
        if r.schema != "sirtone.b3/1" {
            return Err(B3Error::Invalid("schema"));
        }
        let names = labels();
        if names.len() != CLASSES {
            return Err(B3Error::Invalid("labels"));
        }
        let weights = |m: &HashMap<String, f32>| -> Result<Weights, B3Error> {
            let mut w: Weights = m
                .iter()
                .map(|(k, v)| {
                    names
                        .get(k.as_str())
                        .map(|&i| (i, *v))
                        .ok_or_else(|| B3Error::Class(k.clone()))
                })
                .collect::<Result<_, _>>()?;
            w.sort_by_key(|x| x.0);
            Ok(w)
        };
        let scalar = |k: &'static str| -> Result<Scalar, B3Error> {
            let s = r.scalars.get(k).ok_or(B3Error::Invalid(k))?;
            Ok(Scalar {
                pos: weights(&s.pos)?,
                neg: weights(&s.neg)?,
                a: s.a.max(1e-6),
                b: s.b.max(1e-6),
            })
        };
        let groups = |g: &GroupsJson| -> Result<Vec<Weights>, B3Error> {
            g.classes.iter().map(&weights).collect()
        };
        let moods: [Weights; 8] = groups(&r.moods)?
            .try_into()
            .map_err(|_| B3Error::Invalid("moods: 8 groups"))?;
        let genres: [Weights; 6] = groups(&r.genres)?
            .try_into()
            .map_err(|_| B3Error::Invalid("genres: 6 groups"))?;
        if r.brightness.lo_hz <= 0.0 || r.brightness.hi_hz <= r.brightness.lo_hz {
            return Err(B3Error::Invalid("brightness"));
        }
        Ok(Self {
            scalars: [
                scalar("energy")?,
                scalar("valence")?,
                scalar("arousal")?,
                scalar("danceability")?,
                scalar("acousticness")?,
                scalar("vocal")?,
                scalar("aggression")?,
            ],
            moods,
            mood_floor: r.moods.floor.max(1e-6),
            genres,
            bright: (r.brightness.lo_hz, r.brightness.hi_hz),
            instrumental_below: r.instrumental_below,
            conf: (r.conf.two_windows, r.conf.one_window),
        })
    }

    /// The vibe of a tagged track; `windows` — how many clips B2 saw.
    pub fn vibe(&self, b2: &B2, windows: usize) -> Vibe {
        let p = &b2.probs;
        let [
            energy,
            valence,
            arousal,
            danceability,
            acousticness,
            vocal,
            aggression,
        ] = self.scalars.each_ref().map(|s| s.eval(p));
        let sum = |w: &Weights| w.iter().map(|&(i, k)| p[i] * k).sum::<f32>();
        let raw_moods = self.moods.each_ref().map(sum);
        let top = raw_moods.iter().copied().fold(self.mood_floor, f32::max);
        let moods = raw_moods.map(|v| (v / top).clamp(0.0, 1.0));
        let raw_genres = self.genres.each_ref().map(sum);
        let total: f32 = raw_genres.iter().sum();
        let genres = if total > 1e-6 {
            raw_genres.map(|v| v / total)
        } else {
            [1.0 / 6.0; 6]
        };
        let (lo, hi) = self.bright;
        let brightness = if b2.centroid_hz > 0.0 {
            ((b2.centroid_hz / lo).log2() / (hi / lo).log2()).clamp(0.0, 1.0)
        } else {
            0.5
        };
        Vibe {
            energy,
            valence,
            arousal,
            danceability,
            acousticness,
            vocal,
            aggression,
            brightness,
            moods,
            genres,
            instrumental: vocal < self.instrumental_below,
            conf: if windows >= 2 {
                self.conf.0
            } else {
                self.conf.1
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::b2::PROJECTED;

    fn b2(classes: &[(&str, f32)]) -> B2 {
        let names = labels();
        let mut probs = vec![0.001f32; CLASSES];
        for (n, v) in classes {
            probs[names[n]] = *v;
        }
        B2 {
            probs,
            embedding: [0.0; PROJECTED],
            centroid_hz: 1500.0,
        }
    }

    #[test]
    fn every_class_name_resolves() {
        let names = labels();
        assert_eq!(names.len(), CLASSES);
        assert_eq!(names["Music"], 137);
        assert_eq!(names["Christmas music"], 273);
        Rules::builtin().expect("rules");
    }

    #[test]
    fn metal_and_lullaby_land_apart() {
        let r = Rules::builtin().expect("rules");
        let metal = r.vibe(
            &b2(&[
                ("Music", 0.9),
                ("Heavy metal", 0.4),
                ("Distortion", 0.3),
                ("Electric guitar", 0.5),
                ("Angry music", 0.1),
                ("Male singing", 0.3),
            ]),
            2,
        );
        let lullaby = r.vibe(
            &b2(&[
                ("Music", 0.9),
                ("Lullaby", 0.3),
                ("Tender music", 0.2),
                ("Piano", 0.5),
                ("Acoustic guitar", 0.2),
            ]),
            2,
        );
        assert!(
            metal.aggression > 0.8 && lullaby.aggression < 0.2,
            "{metal:?} {lullaby:?}"
        );
        assert!(metal.energy > lullaby.energy + 0.5);
        assert!(lullaby.acousticness > 0.7 && metal.acousticness < 0.3);
        assert!(metal.vocal > 0.6 && lullaby.instrumental);
        assert_eq!(
            metal.genres.iter().copied().fold(0.0, f32::max),
            metal.genres[1]
        );
        assert!((metal.genres.iter().sum::<f32>() - 1.0).abs() < 1e-4);
        // Lullaby: «спокойное» and «нежное» lead.
        assert!(lullaby.moods[2] > 0.5 && lullaby.moods[5] > 0.5 && lullaby.moods[4] < 0.1);
    }

    #[test]
    fn no_mood_evidence_stays_low() {
        let r = Rules::builtin().expect("rules");
        let v = r.vibe(&b2(&[("Music", 0.9)]), 1);
        assert!(v.moods.iter().all(|m| *m < 0.1), "{:?}", v.moods);
        assert!((v.conf - 0.8).abs() < 1e-6);
    }
}
