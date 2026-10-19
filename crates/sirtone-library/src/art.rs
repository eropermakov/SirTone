//! Cover placeholders (T3.20, FR-UI-006, `01b` F-12, D-89): an album or track without art gets
//! a generated placeholder by its genre group and mood instead of a grey note. Here only the
//! hint is computed — one byte, `genre << 4 | mood`: genre group 0…5 (`14` §4.1: поп/танц. ·
//! рок/метал · электроника · хип-хоп/R&B · классика/джаз/эмбиент · фолк/прочее) from the tags
//! (`assets/wave/genres.json`, as the wave reads them), else from the tagger (B3), 7 — unknown;
//! mood 1 весёлое · 2 бодрое · 3 спокойное · 4 грустное, 0 — unknown. The look is the UI's.

use std::collections::HashMap;
use std::sync::OnceLock;

use rusqlite::Connection;
use sirtone_core::text::norm;

use crate::error::LibraryError;

pub const UNKNOWN_GENRE: u8 = 7;
/// The tagger's genre share that counts as «this genre».
const MODEL_SHARE: f32 = 0.4;
const GENRE_AT: usize = 26;
const GENRES: usize = 6;

fn groups() -> &'static [Vec<String>] {
    static G: OnceLock<Vec<Vec<String>>> = OnceLock::new();
    G.get_or_init(|| {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../../assets/wave/genres.json"))
                .unwrap_or_default();
        v["groups"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|g| {
                g.as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|w| w.as_str().and_then(norm))
                    .collect()
            })
            .collect()
    })
}

/// Genre tag → group (first hit on the normalised words, as the wave does).
fn tag_group(genre: &str) -> Option<usize> {
    let g = norm(genre)?;
    groups()
        .iter()
        .position(|words| words.iter().any(|w| g.contains(w.as_str())))
}

/// `track.mood_bits` → 1 весёлое · 2 бодрое · 3 спокойное · 4 грустное · 0.
fn mood_of(bits: i64) -> u8 {
    [1i64, 2, 4, 8]
        .iter()
        .position(|b| bits & b != 0)
        .map_or(0, |i| i as u8 + 1)
}

/// What one or several tracks say.
#[derive(Default)]
struct Votes {
    tags: [u32; GENRES],
    model: [f32; GENRES],
    analysed: u32,
    moods: [u32; 5],
}

impl Votes {
    fn add(&mut self, genre: Option<String>, mood_bits: i64, vec: Option<Vec<u8>>) {
        if let Some(g) = genre.as_deref().and_then(tag_group) {
            self.tags[g] += 1;
        }
        if let Some(v) = vec.filter(|v| v.len() >= (GENRE_AT + GENRES) * 4) {
            for (k, m) in self.model.iter_mut().enumerate() {
                let at = (GENRE_AT + k) * 4;
                *m += f32::from_le_bytes([v[at], v[at + 1], v[at + 2], v[at + 3]]);
            }
            self.analysed += 1;
        }
        self.moods[usize::from(mood_of(mood_bits))] += 1;
    }

    fn code(&self) -> u8 {
        let top = |xs: &[u32]| {
            xs.iter()
                .enumerate()
                .filter(|(_, n)| **n > 0)
                .max_by_key(|(i, n)| (**n, usize::MAX - i))
                .map(|(i, _)| i)
        };
        let genre = top(&self.tags).or_else(|| {
            (self.analysed > 0)
                .then(|| {
                    self.model
                        .iter()
                        .enumerate()
                        .max_by(|a, b| a.1.total_cmp(b.1))
                        .filter(|(_, s)| **s / self.analysed as f32 >= MODEL_SHARE)
                        .map(|(i, _)| i)
                })
                .flatten()
        });
        let mood = top(&self.moods[1..]).map_or(0, |i| i as u8 + 1);
        (genre.map_or(UNKNOWN_GENRE, |g| g as u8) << 4) | mood
    }
}

fn hints(conn: &Connection, ids: &[i64], sql: &str) -> Result<HashMap<i64, u8>, LibraryError> {
    let mut st = conn.prepare_cached(sql)?;
    let mut out = HashMap::with_capacity(ids.len());
    for &id in ids {
        let mut v = Votes::default();
        let mut q = st.query([id])?;
        while let Some(r) = q.next()? {
            v.add(
                r.get(0)?,
                r.get::<_, Option<i64>>(1)?.unwrap_or(0),
                r.get(2)?,
            );
        }
        out.insert(id, v.code());
    }
    Ok(out)
}

/// Placeholder hints of albums (the most common tag genre of their tracks, else the tagger's).
pub fn album_hints(conn: &Connection, ids: &[i64]) -> Result<HashMap<i64, u8>, LibraryError> {
    hints(
        conn,
        ids,
        "SELECT t.genre_main, t.mood_bits, tf.vec FROM track t
         LEFT JOIN track_features tf ON tf.track_id = t.id
         WHERE t.album_id = ?1 AND t.state = 'ok'",
    )
}

/// Placeholder hints of tracks.
pub fn track_hints(conn: &Connection, ids: &[i64]) -> Result<HashMap<i64, u8>, LibraryError> {
    hints(
        conn,
        ids,
        "SELECT t.genre_main, t.mood_bits, tf.vec FROM track t
         LEFT JOIN track_features tf ON tf.track_id = t.id WHERE t.id = ?1",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec_with(genres: [f32; GENRES]) -> Vec<u8> {
        let mut x = [0.5f32; 32];
        x[GENRE_AT..GENRE_AT + GENRES].copy_from_slice(&genres);
        x.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn tags_first_then_the_tagger_then_unknown() {
        let mut v = Votes::default();
        v.add(
            Some("Hard Rock".into()),
            8,
            Some(vec_with([0.0, 0.0, 1.0, 0.0, 0.0, 0.0])),
        );
        v.add(None, 8, None);
        // rock by tag, sad.
        assert_eq!(v.code(), (1 << 4) | 4);
        let mut m = Votes::default();
        m.add(None, 1, Some(vec_with([0.0, 0.0, 0.9, 0.1, 0.0, 0.0])));
        // electronic by the model, happy.
        assert_eq!(m.code(), (2 << 4) | 1);
        let mut u = Votes::default();
        u.add(None, 0, Some(vec_with([0.2, 0.2, 0.2, 0.2, 0.1, 0.1])));
        // no clear genre, no mood: the base placeholder.
        assert_eq!(u.code(), UNKNOWN_GENRE << 4);
        assert_eq!(tag_group("Поп"), Some(0));
        assert_eq!(tag_group("audiobook"), None);
    }
}
