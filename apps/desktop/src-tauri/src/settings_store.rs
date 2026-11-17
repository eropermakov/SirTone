//! Settings service (T0.6, docs/03 §9, docs/12 §4.8): `settings.json` in the data dir,
//! atomic writes (temp + rename) debounced to at most one per second, corrupt file → defaults
//! (the broken file is kept as `settings.json.corrupt-<ms>`), `settings://changed` on every patch.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::settings;
use tauri::{AppHandle, Emitter, State};

use crate::ipc::Json;

const FILE: &str = "settings.json";
const DEBOUNCE: Duration = Duration::from_secs(1);

pub struct SettingsStore {
    doc: Mutex<Value>,
    writer: Sender<Value>,
    path: PathBuf,
}

impl SettingsStore {
    /// Loads (or creates) the settings and starts the debounced writer thread.
    pub fn open(dir: &Path) -> Arc<Self> {
        let path = dir.join(FILE);
        let doc = load(&path);
        let (tx, rx) = mpsc::channel::<Value>();
        let writer_path = path.clone();
        std::thread::Builder::new()
            .name("settings-writer".into())
            .spawn(move || {
                // Coalesce bursts: write the latest document at most once per DEBOUNCE.
                while let Ok(mut latest) = rx.recv() {
                    loop {
                        match rx.recv_timeout(DEBOUNCE) {
                            Ok(newer) => latest = newer,
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                    }
                    if let Err(e) = write_atomic(&writer_path, &latest) {
                        tracing::warn!(target: "sirtone::settings", error = %e, "settings write failed");
                    }
                }
            })
            .ok();
        Arc::new(Self {
            doc: Mutex::new(doc),
            writer: tx,
            path,
        })
    }

    pub fn get(&self) -> Value {
        self.doc.lock().map(|d| d.clone()).unwrap_or(Value::Null)
    }

    /// The preferences that describe a person's local space. Device-wide controls (audio
    /// endpoint, hotkeys, window, performance and privacy switches) deliberately stay outside
    /// this envelope. The library identity is also owned by `spaces.json`, never copied here.
    pub fn space_preferences(&self) -> Value {
        Self::select_space_preferences(self.get())
    }

    pub fn default_space_preferences() -> Value {
        Self::select_space_preferences(settings::default_document())
    }

    fn select_space_preferences(doc: Value) -> Value {
        let mut out = serde_json::Map::new();
        for key in ["appearance", "library", "wave"] {
            if let Some(value) = doc.get(key).cloned() {
                out.insert(key.to_owned(), value);
            }
        }
        if let Some(library) = out.get_mut("library").and_then(Value::as_object_mut) {
            library.remove("root");
            library.remove("uid");
        }
        Value::Object(out)
    }

    /// Replaces the active space's personal envelope while preserving device-wide controls.
    /// The writer still persists the active document as a crash-safe cache; `Spaces` owns the
    /// durable per-space copies and captures it before every switch.
    pub fn apply_space_preferences(
        &self,
        preferences: &Value,
    ) -> Result<Vec<String>, settings::SettingsError> {
        let mut guard = self
            .doc
            .lock()
            .map_err(|_| settings::SettingsError::NotAnObject)?;
        let (next, _typed, changed) = settings::apply_patch(&guard, preferences)?;
        *guard = next.clone();
        let _ = self.writer.send(next);
        Ok(changed)
    }

    /// Like [`Self::patch`], but the file is on disk when it returns — for settings that must
    /// survive a crash right after the change (the library folder: losing it orphans an import).
    pub fn patch_durable(
        &self,
        patch: &Value,
    ) -> Result<(Value, Vec<String>), settings::SettingsError> {
        let (doc, changed) = self.patch(patch)?;
        if let Err(e) = write_atomic(&self.path, &doc) {
            tracing::warn!(target: "sirtone::settings", error = %e, "durable settings write failed");
        }
        Ok((doc, changed))
    }

    pub fn patch(&self, patch: &Value) -> Result<(Value, Vec<String>), settings::SettingsError> {
        let mut guard = self
            .doc
            .lock()
            .map_err(|_| settings::SettingsError::NotAnObject)?;
        let (next, _typed, changed) = settings::apply_patch(&guard, patch)?;
        *guard = next.clone();
        let _ = self.writer.send(next.clone());
        Ok((next, changed))
    }
}

fn load(path: &Path) -> Value {
    let stored = match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<Value>(&text).ok(),
        Err(_) => Some(Value::Object(Default::default())), // first run
    };
    match stored.as_ref().map(settings::normalize) {
        Some(Ok((doc, _))) => doc,
        _ => {
            // Corrupt or invalid: keep the evidence, fall back to defaults (T0.6 acceptance).
            let ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let _ = fs::rename(path, path.with_extension(format!("json.corrupt-{ms}")));
            tracing::warn!(target: "sirtone::settings", "settings file invalid — defaults restored");
            settings::default_document()
        }
    }
}

/// temp file + rename: a crash never leaves a half-written settings.json.
pub fn write_atomic(path: &Path, doc: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp: PathBuf = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(doc).map_err(std::io::Error::other)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

#[tauri::command]
#[specta::specta]
pub fn settings_get(store: State<'_, Arc<SettingsStore>>) -> Json {
    Json(store.get())
}

#[tauri::command(async)]
#[specta::specta]
pub fn settings_patch(
    app: AppHandle,
    store: State<'_, Arc<SettingsStore>>,
    patch: Json,
) -> Result<Json, AppError> {
    let (doc, changed) = store
        .patch(&patch.0)
        .map_err(|e| AppError::new(ErrorCode::InvalidArgument).with_details(e))?;
    if changed.iter().any(|k| k == "hotkeys") {
        crate::hotkeys::apply(&app); // FR-WIN-001: applied without a restart
    }
    if changed.iter().any(|k| k == "playback" || k == "sfx")
        && let Some(pb) = tauri::Manager::try_state::<crate::playback::Playback>(&app)
    {
        pb.renormalize(); // T4.11: a new target or the switch — heard at once
    }
    if changed.iter().any(|k| k == "widget" || k == "appearance") {
        crate::widget::apply(&app); // T5.1: on/off, «поверх окон», size, the look
    }
    if changed.iter().any(|k| k == "library") {
        crate::library::ensure_inbox(&app); // T6.8: «Папка Inbox» appears at once
    }
    if changed.iter().any(|k| k == "language") {
        crate::i18n::apply(&app); // T6.5: the tray, the taskbar, the widget speak it at once
    }
    if changed.iter().any(|k| k == "events") {
        crate::events::kick(); // T5.13: switches apply at once
    }
    if changed.iter().any(|k| k == "context") {
        crate::context::apply(&app); // T5.5: a new city — the sun and the weather follow
    } else if changed.iter().any(|k| k == "wave") {
        crate::context::rebuild(&app); // FR-SEAS-001: «учитывать погоду», no new fetch
    }
    if changed.iter().any(|k| k == "network") {
        crate::net::apply(&app); // T3.1: the offline switch, at once
    }
    let _ = app.emit("settings://changed", &changed);
    Ok(Json(doc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(name: &str) -> PathBuf {
        let p =
            std::env::temp_dir().join(format!("sirtone-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults_and_is_kept() {
        let dir = tmp("corrupt");
        fs::write(dir.join(FILE), "{ not json").unwrap();
        let doc = load(&dir.join(FILE));
        assert_eq!(doc, settings::default_document());
        let kept = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(kept);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn patch_is_written_atomically_after_debounce() {
        let dir = tmp("write");
        let store = SettingsStore::open(&dir);
        store
            .patch(&json!({ "appearance": { "theme": "light" } }))
            .unwrap();
        store
            .patch(&json!({ "appearance": { "accent": { "preset": "blue" } } }))
            .unwrap();
        std::thread::sleep(DEBOUNCE + Duration::from_millis(600));
        let on_disk: Value =
            serde_json::from_str(&fs::read_to_string(dir.join(FILE)).unwrap()).unwrap();
        assert_eq!(on_disk["appearance"]["theme"], "light");
        assert_eq!(on_disk["appearance"]["accent"]["preset"], "blue");
        assert!(!dir.join("settings.json.tmp").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_patch_changes_nothing() {
        let dir = tmp("invalid");
        let store = SettingsStore::open(&dir);
        let before = store.get();
        assert!(
            store
                .patch(&json!({ "performance": { "level": 99 } }))
                .is_err()
        );
        assert_eq!(store.get(), before);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn space_preferences_keep_personal_settings_but_not_library_identity() {
        let prefs = SettingsStore::select_space_preferences(json!({
            "appearance": { "theme": "dark" },
            "library": { "root": "D:/Music", "uid": "library-a", "import_quality": "compact" },
            "wave": { "enabled": true },
            "window": { "autostart": true }
        }));
        assert_eq!(prefs["appearance"]["theme"], "dark");
        assert_eq!(prefs["library"]["import_quality"], "compact");
        assert!(prefs["library"].get("root").is_none());
        assert!(prefs["library"].get("uid").is_none());
        assert!(prefs.get("window").is_none());
    }
}
