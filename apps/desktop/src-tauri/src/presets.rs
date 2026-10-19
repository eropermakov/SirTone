//! MilkDrop preset ratings (T4.3, FR-VIZ-004 — Kodi «RatePreset», `01a` §B.2): ♥ (+1) draws a
//! preset closer in the vibe pick, ✕ (−1) removes it. Kept in the data folder
//! (`viz-presets.json`, `{name: rating}`), not in a library: they belong to the person, not to a
//! music folder.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use sirtone_core::error::{AppError, ErrorCode};
use tauri::State;

pub struct Presets {
    path: PathBuf,
    ratings: Mutex<BTreeMap<String, i8>>,
}

impl Presets {
    pub fn open(data_dir: &std::path::Path) -> Self {
        let path = data_dir.join("viz-presets.json");
        let ratings = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            ratings: Mutex::new(ratings),
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct PresetRating {
    pub name: String,
    pub rating: i8,
}

#[tauri::command]
#[specta::specta]
pub fn viz_preset_ratings(presets: State<'_, Presets>) -> Vec<PresetRating> {
    presets
        .ratings
        .lock()
        .map(|r| {
            r.iter()
                .map(|(name, rating)| PresetRating {
                    name: name.clone(),
                    rating: *rating,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Rates a preset: 1 ♥, −1 ✕, 0 — no opinion (forgotten).
#[tauri::command]
#[specta::specta]
pub fn viz_preset_rate(
    presets: State<'_, Presets>,
    name: String,
    rating: i8,
) -> Result<(), AppError> {
    if name.is_empty() || name.len() > 300 || !(-1..=1).contains(&rating) {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    let mut r = presets
        .ratings
        .lock()
        .map_err(|_| AppError::new(ErrorCode::Internal))?;
    if rating == 0 {
        r.remove(&name);
    } else {
        r.insert(name, rating);
    }
    let doc = serde_json::to_value(&*r)
        .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))?;
    crate::settings_store::write_atomic(&presets.path, &doc)
        .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratings_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("sirtone-presets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = Presets::open(&dir);
        p.ratings
            .lock()
            .unwrap()
            .insert("Geiss - Cosmic Dust 2".into(), 1);
        let doc = serde_json::to_value(&*p.ratings.lock().unwrap()).unwrap();
        crate::settings_store::write_atomic(&p.path, &doc).unwrap();
        let again = Presets::open(&dir);
        assert_eq!(
            again.ratings.lock().unwrap().get("Geiss - Cosmic Dust 2"),
            Some(&1)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
