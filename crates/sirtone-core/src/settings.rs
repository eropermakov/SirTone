//! Settings schema v1 (docs/03 §9, docs/12 §4.8).
//!
//! The document on disk is kept as JSON so **unknown fields survive** (forward compatibility);
//! the typed [`Settings`] view validates it and supplies defaults for anything missing.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const SETTINGS_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub settings_version: u32,
    pub language: String,
    pub appearance: Appearance,
    pub performance: Performance,
    pub playback: Playback,
    pub viz: Viz,
    pub fx: Fx,
    pub wave: Wave,
    pub events: Events,
    pub sleep: Sleep,
    pub focus: Focus,
    pub sfx: Sfx,
    pub window: WindowPrefs,
    pub library: LibraryPrefs,
    pub network: Network,
    pub context: ContextPrefs,
    pub updates: Updates,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            settings_version: SETTINGS_VERSION,
            language: "ru".into(),
            appearance: Appearance::default(),
            performance: Performance::default(),
            playback: Playback::default(),
            viz: Viz::default(),
            fx: Fx::default(),
            wave: Wave::default(),
            events: Events::default(),
            sleep: Sleep::default(),
            focus: Focus::default(),
            sfx: Sfx::default(),
            window: WindowPrefs::default(),
            library: LibraryPrefs::default(),
            network: Network::default(),
            context: ContextPrefs::default(),
            updates: Updates::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccentSource {
    Preset,
    Auto,
    Custom,
    Seasonal,
}

/// The 17 owner colour sets (DESIGN.md §4).
pub const ACCENT_PRESETS: [&str; 17] = [
    "sirkov", "blue", "violet", "lilac", "lavender", "gray", "black", "yellow", "orange", "ginger",
    "coral", "brick", "lightred", "red", "darkred", "burgundy", "bordeaux",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Accent {
    pub source: AccentSource,
    pub preset: String,
    /// «Свой» — only one colour is stored (FR-UI-003).
    pub custom: Option<String>,
}

impl Default for Accent {
    fn default() -> Self {
        Self {
            source: AccentSource::Preset,
            preset: "sirkov".into(),
            custom: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemePref,
    pub accent: Accent,
    pub custom_accents: Vec<String>,
    pub seasonal_theme: bool,
    pub event_themes_auto: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemePref::System,
            accent: Accent::default(),
            custom_accents: Vec::new(),
            seasonal_theme: false,
            event_themes_auto: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Performance {
    pub level: u8,
    pub auto_degrade: bool,
    pub degraded_from: Option<u8>,
    pub overrides: Map<String, Value>,
}

impl Default for Performance {
    fn default() -> Self {
        Self {
            level: 3,
            auto_degrade: true,
            degraded_from: None,
            overrides: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Playback {
    /// Player slider 0…1 (cubic curve in the engine), remembered between launches (D-64).
    pub volume: f32,
    pub automix: Automix,
    pub normalization: Normalization,
    pub eq: Eq,
    pub output: Output,
    pub duck: Duck,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Automix {
    pub mode: String,
    pub intensity: String,
    pub crossfade_s: f32,
    pub skip_albums: bool,
    pub genre_blacklist: Vec<String>,
}

impl Default for Automix {
    fn default() -> Self {
        Self {
            mode: "smart".into(),
            intensity: "medium".into(),
            crossfade_s: 6.0,
            skip_albums: true,
            genre_blacklist: ["classical", "live", "ambient", "audiobook"]
                .map(String::from)
                .to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Normalization {
    pub enabled: bool,
    pub target_lufs: f32,
    pub album_mode: String,
}

impl Default for Normalization {
    fn default() -> Self {
        Self {
            enabled: true,
            target_lufs: -14.0,
            album_mode: "auto".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Eq {
    pub enabled: bool,
    pub preamp_db: f32,
    pub bands_db: Vec<f32>,
    pub preset: String,
}

impl Default for Eq {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            bands_db: vec![0.0; 10],
            preset: "flat".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Output {
    pub device: String,
    pub exclusive: bool,
    pub pause_on_device_loss: bool,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            device: "default".into(),
            exclusive: false,
            pause_on_device_loss: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Duck {
    pub enabled: bool,
    pub amount_db: f32,
    pub sensitivity: String,
    pub ignore: Vec<String>,
    pub pause_on_call: bool,
}

impl Default for Duck {
    fn default() -> Self {
        Self {
            enabled: true,
            amount_db: -12.0,
            sensitivity: "medium".into(),
            ignore: Vec::new(),
            pause_on_call: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Viz {
    pub mode: String,
    pub follow_theme_accent: bool,
}

impl Default for Viz {
    fn default() -> Self {
        Self {
            mode: "waves".into(),
            follow_theme_accent: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fx {
    pub mode: String,
    pub manual: Option<String>,
    pub static_when_weak: bool,
}

impl Default for Fx {
    fn default() -> Self {
        Self {
            mode: "weather".into(),
            manual: None,
            static_when_weak: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wave {
    pub learning: bool,
    pub novelty: u8,
    pub repeat_window_h: u32,
    pub artist_spacing: u32,
    pub season: Value,
    pub influence: Value,
}

impl Default for Wave {
    fn default() -> Self {
        Self {
            learning: true,
            novelty: 50,
            repeat_window_h: 24,
            artist_spacing: 5,
            season: serde_json::json!({ "mode": "auto", "weather_nudge": true, "blend_days": 21 }),
            influence: serde_json::json!({ "season": 1.0, "context": 1.0, "events": 1.0 }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Events {
    pub suggest: bool,
    pub auto_play: bool,
    pub religious: bool,
    pub micro: Value,
}

impl Default for Events {
    fn default() -> Self {
        Self {
            suggest: true,
            auto_play: false,
            religious: true,
            micro: serde_json::json!({ "weather": true, "weekly": false, "astro": true }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sleep {
    pub fade_min: u32,
    pub final_s: u32,
    pub warm: bool,
    pub after: String,
}

impl Default for Sleep {
    fn default() -> Self {
        Self {
            fade_min: 10,
            final_s: 30,
            warm: false,
            after: "nothing".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Focus {
    pub work_min: u32,
    pub break_min: u32,
    pub long_break_min: u32,
    pub cycles_before_long: u32,
    pub vocals: String,
    pub overrun_s: u32,
    pub auto_normalize: bool,
}

impl Default for Focus {
    fn default() -> Self {
        Self {
            work_min: 25,
            break_min: 5,
            long_break_min: 15,
            cycles_before_long: 4,
            vocals: "prefer_none".into(),
            overrun_s: 90,
            auto_normalize: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sfx {
    pub enabled: bool,
    pub volume: f32,
}

impl Default for Sfx {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowPrefs {
    pub start_mode: String,
    pub close_to_tray: bool,
    pub autostart: bool,
    pub keep_in_memory: bool,
}

impl Default for WindowPrefs {
    fn default() -> Self {
        Self {
            start_mode: "maximized".into(),
            close_to_tray: true,
            autostart: false,
            keep_in_memory: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryPrefs {
    pub root: Option<String>,
    pub adopt_external_files: bool,
    pub organize_external: String,
}

impl Default for LibraryPrefs {
    fn default() -> Self {
        Self {
            root: None,
            adopt_external_files: true,
            organize_external: "suggest".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Network {
    pub offline_mode: bool,
    pub geo: Geo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Geo {
    pub mode: String,
    pub city: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

impl Default for Geo {
    fn default() -> Self {
        Self {
            mode: "manual".into(),
            city: None,
            lat: None,
            lon: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextPrefs {
    pub auto_atmosphere: bool,
    pub activity_detection: bool,
}

impl Default for ContextPrefs {
    fn default() -> Self {
        Self {
            auto_atmosphere: true,
            activity_detection: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Updates {
    pub auto_check: bool,
}

impl Default for Updates {
    fn default() -> Self {
        Self { auto_check: true }
    }
}

/// Why a settings document or patch was rejected.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings document is not a JSON object")]
    NotAnObject,
    #[error("invalid settings: {0}")]
    Invalid(String),
}

/// Recursively merges `patch` into `base` (objects merge, everything else replaces).
pub fn deep_merge(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                deep_merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (slot, v) => *slot = v.clone(),
    }
}

/// Validates the semantic rules serde types cannot express.
impl Default for Playback {
    fn default() -> Self {
        Self {
            volume: 1.0,
            automix: Automix::default(),
            normalization: Normalization::default(),
            eq: Eq::default(),
            output: Output::default(),
            duck: Duck::default(),
        }
    }
}

fn validate(s: &Settings) -> Result<(), SettingsError> {
    let a = &s.appearance.accent;
    if !ACCENT_PRESETS.contains(&a.preset.as_str()) {
        return Err(SettingsError::Invalid(format!(
            "unknown accent preset `{}`",
            a.preset
        )));
    }
    if a.source == AccentSource::Custom && a.custom.as_deref().is_none_or(|c| !is_hex_color(c)) {
        return Err(SettingsError::Invalid(
            "custom accent needs a #rrggbb colour".into(),
        ));
    }
    if s.performance.level > 4 {
        return Err(SettingsError::Invalid(
            "performance.level must be 0..=4".into(),
        ));
    }
    if !(0.0..=1.0).contains(&s.playback.volume) {
        return Err(SettingsError::Invalid(
            "playback.volume must be 0..=1".into(),
        ));
    }
    if s.wave.novelty > 100 {
        return Err(SettingsError::Invalid(
            "wave.novelty must be 0..=100".into(),
        ));
    }
    Ok(())
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Defaults as a JSON document.
pub fn default_document() -> Value {
    serde_json::to_value(Settings::default()).unwrap_or(Value::Object(Map::new()))
}

/// Builds the effective document: defaults ⊕ stored. Unknown stored fields are kept.
pub fn normalize(stored: &Value) -> Result<(Value, Settings), SettingsError> {
    if !stored.is_object() {
        return Err(SettingsError::NotAnObject);
    }
    let mut doc = default_document();
    deep_merge(&mut doc, stored);
    let typed: Settings =
        serde_json::from_value(doc.clone()).map_err(|e| SettingsError::Invalid(e.to_string()))?;
    validate(&typed)?;
    Ok((doc, typed))
}

/// Applies a patch to an effective document; returns the new document and the top-level keys changed.
pub fn apply_patch(
    doc: &Value,
    patch: &Value,
) -> Result<(Value, Settings, Vec<String>), SettingsError> {
    let Value::Object(p) = patch else {
        return Err(SettingsError::NotAnObject);
    };
    if p.contains_key("settings_version") {
        return Err(SettingsError::Invalid(
            "settings_version is managed by the app".into(),
        ));
    }
    let mut next = doc.clone();
    deep_merge(&mut next, patch);
    let (next, typed) = normalize(&next)?;
    let changed = p
        .keys()
        .filter(|k| doc.get(*k) != next.get(*k))
        .cloned()
        .collect();
    Ok((next, typed, changed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.appearance.theme, ThemePref::System);
        assert_eq!(s.appearance.accent.preset, "sirkov");
        assert!((s.playback.normalization.target_lufs + 14.0).abs() < f32::EPSILON);
        assert_eq!(s.wave.repeat_window_h, 24);
    }

    #[test]
    fn unknown_fields_survive_and_missing_get_defaults() {
        let stored = json!({ "future_feature": { "x": 1 }, "appearance": { "theme": "light" } });
        let (doc, typed) = normalize(&stored).unwrap();
        assert_eq!(doc["future_feature"]["x"], 1);
        assert_eq!(typed.appearance.theme, ThemePref::Light);
        assert_eq!(typed.appearance.accent.preset, "sirkov");
    }

    #[test]
    fn patch_reports_changed_keys_and_rejects_invalid() {
        let (doc, _) = normalize(&json!({})).unwrap();
        let (_, typed, changed) = apply_patch(
            &doc,
            &json!({ "appearance": { "accent": { "source": "custom", "custom": "#22aa66" } } }),
        )
        .unwrap();
        assert_eq!(changed, vec!["appearance".to_string()]);
        assert_eq!(typed.appearance.accent.custom.as_deref(), Some("#22aa66"));
        assert!(
            apply_patch(
                &doc,
                &json!({ "appearance": { "accent": { "preset": "pink" } } })
            )
            .is_err()
        );
        assert!(apply_patch(&doc, &json!({ "performance": { "level": 9 } })).is_err());
        assert!(apply_patch(&doc, &json!({ "appearance": { "theme": "neon" } })).is_err());
        assert!(apply_patch(&doc, &json!({ "settings_version": 7 })).is_err());
    }
}
