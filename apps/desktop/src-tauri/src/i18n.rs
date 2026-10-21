//! The UI strings Rust shows itself (T6.5, FR-I18N-001): the tray, the taskbar buttons and the
//! Jump List, the desktop widget — they work while the WebView is gone, so they read the same
//! string files as the UI, in the language of settings `language` (`ru` · `en`). English is a
//! slot (FR-I18N-001): every key it lacks falls back to Russian, key by key.

use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::settings_store::SettingsStore;

const RU_SHELL: &str = include_str!("../../../../assets/i18n/ru/shell.json");
const EN_SHELL: &str = include_str!("../../../../assets/i18n/en/shell.json");

pub const LANGUAGES: [&str; 2] = ["ru", "en"];

/// `over` on top of `base`, object by object (a missing key keeps the base's).
fn overlay(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(slot) if slot.is_object() && v.is_object() => overlay(slot, v),
                    _ if v.as_str().is_some_and(str::is_empty) => {}
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (slot, v) => *slot = v.clone(),
    }
}

/// The `shell` strings of a language (cached).
pub fn shell_of(lang: &str) -> Arc<Value> {
    static CACHE: Mutex<Vec<(String, Arc<Value>)>> = Mutex::new(Vec::new());
    let mut c = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((_, v)) = c.iter().find(|(l, _)| l == lang) {
        return v.clone();
    }
    let mut doc: Value = serde_json::from_str(RU_SHELL).unwrap_or_default();
    if lang == "en"
        && let Ok(en) = serde_json::from_str::<Value>(EN_SHELL)
    {
        overlay(&mut doc, &en);
    }
    let v = Arc::new(doc);
    c.push((lang.to_owned(), v.clone()));
    v
}

/// The language of the settings (`ru` when unknown).
pub fn lang(app: &AppHandle) -> String {
    app.try_state::<Arc<SettingsStore>>()
        .and_then(|s| s.get()["language"].as_str().map(str::to_owned))
        .filter(|l| LANGUAGES.contains(&l.as_str()))
        .unwrap_or_else(|| "ru".into())
}

/// The `shell` strings in the app's language.
pub fn shell(app: &AppHandle) -> Arc<Value> {
    shell_of(&lang(app))
}

/// The language changed: everything Rust labels itself follows.
pub fn apply(app: &AppHandle) {
    crate::tray::relabel(app);
    crate::taskbar::relabel(app);
    crate::widget::relabel(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_falls_back_to_russian_key_by_key() {
        let ru = shell_of("ru");
        let en = shell_of("en");
        // A key both have: English; a key only Russian has: Russian.
        assert_eq!(en["tray"]["next"].as_str(), Some("Next"));
        assert_eq!(ru["tray"]["next"].as_str(), Some("Вперёд"));
        // An empty English string is «not translated yet».
        assert_eq!(en["tray"]["quit"], ru["tray"]["quit"]);
        assert_eq!(en["widget"]["sleep30"], ru["widget"]["sleep30"]);
        // Nothing the Russian file has is missing in English.
        fn count(v: &Value) -> usize {
            match v {
                Value::Object(m) => m.values().map(count).sum(),
                _ => 1,
            }
        }
        assert_eq!(count(&en), count(&ru));
    }
}
