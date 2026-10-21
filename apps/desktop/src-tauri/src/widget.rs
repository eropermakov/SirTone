//! The desktop widget window (T5.1, FR-WIN-003/005, `07` §5, D-18): a frameless transparent
//! window without a taskbar button that never takes focus and stays under every other window
//! (strategy A of S-09: bottom-most top-level — the «Rainmeter On Desktop» way; B, parenting
//! into Progman/WorkerW, changes the shell's own windows and is not used). «Поверх окон» is the
//! user's option C. The card is dragged by its background; the place is remembered (per
//! monitor, through the window position); scale 0.75–1.5 is the WebView zoom. Settings
//! `widget.*`; the window comes and goes with `widget.enabled`.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
};

use crate::settings_store::SettingsStore;

const LABEL: &str = "widget";
/// The card at scale 1 (logical px), with the transparent margin for the shadow.
const W: f64 = 380.0;
const H: f64 = 128.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Prefs {
    enabled: bool,
    on_top: bool,
    scale: f64,
    pos: Option<(i32, i32)>,
}

fn prefs(app: &AppHandle) -> Prefs {
    let doc = app
        .try_state::<Arc<SettingsStore>>()
        .map(|s| s.get())
        .unwrap_or_default();
    let w = &doc["widget"];
    Prefs {
        enabled: w["enabled"].as_bool().unwrap_or(false),
        on_top: w["on_top"].as_bool().unwrap_or(false),
        scale: w["scale"].as_f64().unwrap_or(1.0).clamp(0.75, 1.5),
        pos: match (w["x"].as_i64(), w["y"].as_i64()) {
            (Some(x), Some(y)) => Some((x as i32, y as i32)),
            _ => None,
        },
    }
}

/// The widget's look: the app's theme and accent preset.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct WidgetState {
    pub theme: String,
    pub accent: String,
    pub scale: f64,
}

fn look(app: &AppHandle) -> WidgetState {
    let doc = app
        .try_state::<Arc<SettingsStore>>()
        .map(|s| s.get())
        .unwrap_or_default();
    let a = &doc["appearance"];
    let theme = match a["theme"].as_str() {
        Some("light") => "light",
        Some("dark") => "dark",
        _ => {
            if sirtone_platform::taskbar::light_taskbar() {
                "light"
            } else {
                "dark"
            }
        }
    };
    let accent = a["accent"]["preset"]
        .as_str()
        .unwrap_or("sirkov")
        .to_owned();
    WidgetState {
        theme: theme.into(),
        accent,
        scale: prefs(app).scale,
    }
}

/// Creates, adjusts or closes the window to match the settings (start, settings change).
pub fn apply(app: &AppHandle) {
    let p = prefs(app);
    let existing = app.get_webview_window(LABEL);
    if !p.enabled {
        if let Some(w) = existing {
            let _ = w.destroy();
        }
        return;
    }
    let size = LogicalSize::new(W * p.scale, H * p.scale);
    if let Some(w) = existing {
        let _ = w.set_size(size);
        let _ = w.set_zoom(p.scale);
        let _ = w.set_always_on_top(p.on_top);
        let _ = w.set_always_on_bottom(!p.on_top);
        let _ = app.emit_to(LABEL, "widget-look", look(app));
        return;
    }
    let b = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("widget.html".into()))
        .title("Сиртон")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .skip_taskbar(true)
        .focused(false)
        .always_on_top(p.on_top)
        .always_on_bottom(!p.on_top)
        .inner_size(size.width, size.height)
        .visible(false);
    match b.build() {
        Ok(w) => {
            if let Some((x, y)) = p.pos {
                let _ = w.set_position(PhysicalPosition::new(x, y));
            } else if let Ok(Some(m)) = w.primary_monitor() {
                // First time: the lower right of the work area.
                let s = m.size();
                let f = m.scale_factor();
                let _ = w.set_position(PhysicalPosition::new(
                    s.width as i32 - ((W * p.scale + 32.0) * f) as i32,
                    s.height as i32 - ((H * p.scale + 96.0) * f) as i32,
                ));
            }
            let _ = w.set_zoom(p.scale);
            let _ = w.show();
        }
        Err(e) => tracing::warn!(target: "sirtone::widget", error = %e, "widget window failed"),
    }
}

/// The language changed (T6.5): the open widget asks for its texts again.
pub fn relabel(app: &AppHandle) {
    let _ = app.emit_to(LABEL, "widget-texts", ());
}

static MOVED: Mutex<Option<Instant>> = Mutex::new(None);

/// The card was dragged: remember the place (debounced 600 ms).
pub fn moved(app: &AppHandle, x: i32, y: i32) {
    if let Ok(mut m) = MOVED.lock() {
        *m = Some(Instant::now());
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        let quiet = MOVED
            .lock()
            .is_ok_and(|m| m.is_some_and(|t| t.elapsed() >= Duration::from_millis(550)));
        if quiet && let Some(st) = app.try_state::<Arc<SettingsStore>>() {
            let _ = st.patch(&serde_json::json!({ "widget": { "x": x, "y": y } }));
        }
    });
}

#[tauri::command]
#[specta::specta]
pub fn widget_state(app: AppHandle) -> WidgetState {
    look(&app)
}

/// The widget's texts in the app's language (from the shell strings — the page carries no i18n
/// bundle).
#[tauri::command]
#[specta::specta]
pub fn widget_texts(app: AppHandle) -> std::collections::BTreeMap<String, String> {
    texts_of(&crate::i18n::lang(&app))
}

fn texts_of(lang: &str) -> std::collections::BTreeMap<String, String> {
    let doc = crate::i18n::shell_of(lang);
    let w = &doc["widget"];
    [
        "prev",
        "play",
        "pause",
        "next",
        "sleep30",
        "sleepLeft",
        "nothing",
        "open",
        "focusWork",
        "focusBreak",
    ]
    .into_iter()
    .map(|k| (k.to_owned(), w[k].as_str().unwrap_or(k).to_owned()))
    .collect()
}

/// A click on the cover: the main window.
#[tauri::command]
#[specta::specta]
pub fn widget_open_main(app: AppHandle) {
    crate::tray::show_main(&app);
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_widget_text_is_there() {
        let t = super::texts_of("ru");
        assert_eq!(t.len(), 10);
        assert!(t.iter().all(|(k, v)| v != k && !v.is_empty()), "{t:?}");
    }
}
