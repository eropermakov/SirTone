//! The visualiser's colours on the phone (D-148): the PC's `viz_colors` (`VizColors`, T4.5) —
//! the track's vibe, the cover's dominant colours and «Цвет вашей музыки» — as JSON in the PC's
//! camelCase shape, so the PC's «Волны» and MilkDrop pages read it unchanged.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::json;
use sirtone_library::palette::{self, Swatch};

use crate::library::Library;

/// «Цвет вашей музыки» is worked out again after this (as the PC's `COLLECTION_TTL`).
const COLLECTION_TTL: Duration = Duration::from_secs(600);

static COLLECTION: Mutex<Option<(Instant, Vec<Swatch>)>> = Mutex::new(None);

fn palette_of(lib: &Library, root: &std::path::Path, hash: &str) -> Vec<Swatch> {
    if let Some(p) = lib.with(|c| palette::cached(c, hash).ok().flatten()) {
        return p;
    }
    // Decoded without the connection's lock; the result is kept for next time.
    let p = palette::of_cover(root, hash).unwrap_or_default();
    if !p.is_empty() {
        let _ = lib.with(|c| palette::store(c, hash, &p));
    }
    p
}

fn vibe(c: &Connection, track_id: i64) -> [Option<f64>; 6] {
    c.query_row(
        "SELECT valence, arousal, energy, aggression, danceability, acousticness
           FROM track WHERE id = ?1",
        [track_id],
        |r| {
            let mut v = [None; 6];
            for (i, x) in v.iter_mut().enumerate() {
                *x = r.get::<_, Option<f64>>(i)?;
            }
            Ok(v)
        },
    )
    .unwrap_or([None; 6])
}

fn swatches(s: &[Swatch]) -> serde_json::Value {
    s.iter()
        .map(|w| json!({ "l": w.l, "c": w.c, "h": w.h, "w": w.w }))
        .collect()
}

#[uniffi::export]
impl Library {
    /// The visualiser's colours for a track (`cover` — its cover hash, if any) as JSON.
    pub fn viz_colors(self: Arc<Self>, track_id: i64, cover: Option<String>) -> String {
        let root = std::path::PathBuf::from(self.root());
        let v = self.with(|c| vibe(c, track_id));
        let cover = cover
            .map(|h| palette_of(&self, &root, &h))
            .unwrap_or_default();
        let fresh = COLLECTION
            .lock()
            .ok()
            .and_then(|c| c.clone())
            .filter(|(at, _)| at.elapsed() < COLLECTION_TTL);
        let collection = match fresh {
            Some((_, c)) => c,
            None => {
                let albums = self
                    .with(|c| palette::collection_covers(c))
                    .unwrap_or_default();
                let palettes: Vec<(Vec<Swatch>, f32)> = albums
                    .iter()
                    .map(|(h, w)| (palette_of(&self, &root, h), *w))
                    .collect();
                let c = palette::collection(&palettes);
                if let Ok(mut slot) = COLLECTION.lock() {
                    *slot = Some((Instant::now(), c.clone()));
                }
                c
            }
        };
        json!({
            "valence": v[0], "arousal": v[1], "energy": v[2], "aggression": v[3],
            "danceability": v[4], "acousticness": v[5],
            "cover": swatches(&cover), "collection": swatches(&collection),
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty library: no vibe, no cover — still valid JSON in the PC's shape.
    #[test]
    fn colours_of_an_unknown_track() {
        let dir = std::env::temp_dir().join(format!("sirtone-viz-{}", std::process::id()));
        let lib = Library::open(dir.to_string_lossy().into_owned()).unwrap();
        let j: serde_json::Value = serde_json::from_str(&lib.clone().viz_colors(1, None)).unwrap();
        assert!(j["valence"].is_null());
        assert_eq!(j["cover"], serde_json::json!([]));
        assert!(j["collection"].is_array());
        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
