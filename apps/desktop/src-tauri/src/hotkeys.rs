//! Global hotkeys (T1.22, FR-WIN-001, docs/07 §2): RegisterHotKey through
//! `tauri-plugin-global-shortcut`. Actions go straight to the Rust playback service (they work with
//! the window hidden or its WebView gone). A combination another program holds is a conflict: an
//! event and a hint in the settings, never an error dialog. Changing the setting re-registers at
//! once — no restart (better than VLC).

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tauri_specta::Event;

use crate::playback::Playback;
use crate::settings_store::SettingsStore;

/// Volume step of the volume hotkeys (docs/07 §2: ±5 %).
const VOLUME_STEP: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum HotkeyAction {
    PlayPause,
    Next,
    Prev,
    VolumeUp,
    VolumeDown,
    Mute,
}

impl HotkeyAction {
    const ALL: [Self; 6] = [
        Self::PlayPause,
        Self::Next,
        Self::Prev,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::Mute,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::PlayPause => "play_pause",
            Self::Next => "next",
            Self::Prev => "prev",
            Self::VolumeUp => "volume_up",
            Self::VolumeDown => "volume_down",
            Self::Mute => "mute",
        }
    }
}

/// One configured hotkey and whether Windows gave it to us.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    pub action: HotkeyAction,
    pub accel: String,
    /// `false` = taken by another program (or not a valid combination).
    pub registered: bool,
}

/// A hotkey could not be registered (FR-WIN-001 «обнаружение конфликтов»).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConflict {
    pub action: HotkeyAction,
    pub accel: String,
}

#[derive(Default)]
pub struct Hotkeys(Mutex<Vec<(Shortcut, HotkeyAction, HotkeyStatus)>>);

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let action = app.try_state::<Hotkeys>().and_then(|h| {
                h.0.lock()
                    .ok()?
                    .iter()
                    .find(|(s, _, _)| s == shortcut)
                    .map(|(_, a, _)| *a)
            });
            if let (Some(action), Some(pb)) = (action, app.try_state::<Playback>()) {
                run(&pb, action);
            }
        })
        .build()
}

fn run(pb: &Playback, action: HotkeyAction) {
    match action {
        HotkeyAction::PlayPause => pb.toggle(),
        HotkeyAction::Next => pb.next(),
        HotkeyAction::Prev => pb.prev(),
        HotkeyAction::VolumeUp => pb.set_volume(stepped(pb.volume(), VOLUME_STEP)),
        HotkeyAction::VolumeDown => pb.set_volume(stepped(pb.volume(), -VOLUME_STEP)),
        HotkeyAction::Mute => pb.toggle_mute(),
    }
}

/// Next volume on a 5 % grid (so repeated presses land on round values).
fn stepped(v: f32, step: f32) -> f32 {
    let steps = (v / step.abs()).round() + step.signum();
    (steps * step.abs()).clamp(0.0, 1.0)
}

/// The configured accelerators (settings `hotkeys`, docs/03 §6).
fn configured(settings: &SettingsStore) -> Vec<(HotkeyAction, String)> {
    let doc = settings.get();
    HotkeyAction::ALL
        .iter()
        .filter_map(|a| {
            doc["hotkeys"][a.key()]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(|s| (*a, s.to_owned()))
        })
        .collect()
}

/// (Re)registers every configured hotkey; conflicts are reported, not fatal.
pub fn apply(app: &AppHandle) {
    let (Some(settings), Some(state)) = (
        app.try_state::<std::sync::Arc<SettingsStore>>(),
        app.try_state::<Hotkeys>(),
    ) else {
        return;
    };
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut table = Vec::new();
    for (action, accel) in configured(&settings) {
        let parsed: Option<Shortcut> = accel.parse().ok();
        let registered = parsed.as_ref().is_some_and(|s| gs.register(*s).is_ok());
        if !registered {
            tracing::warn!(target: "sirtone::shell", ?action, accel = %accel, "hotkey taken");
            let _ = HotkeyConflict {
                action,
                accel: accel.clone(),
            }
            .emit(app);
        }
        if let Some(s) = parsed {
            table.push((
                s,
                action,
                HotkeyStatus {
                    action,
                    accel,
                    registered,
                },
            ));
        }
    }
    *state.0.lock().unwrap_or_else(|p| p.into_inner()) = table;
}

/// Settings → «Горячие клавиши»: what is configured and what Windows refused.
#[tauri::command]
#[specta::specta]
pub fn hotkeys_status(app: AppHandle) -> Vec<HotkeyStatus> {
    app.try_state::<Hotkeys>()
        .map(|h| {
            h.0.lock()
                .map(|t| t.iter().map(|(_, _, s)| s.clone()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_steps_on_a_five_percent_grid() {
        assert!((stepped(0.5, VOLUME_STEP) - 0.55).abs() < 1e-6);
        assert!((stepped(0.37, VOLUME_STEP) - 0.40).abs() < 1e-6);
        assert!((stepped(0.37, -VOLUME_STEP) - 0.30).abs() < 1e-6);
        assert_eq!(stepped(1.0, VOLUME_STEP), 1.0);
        assert_eq!(stepped(0.02, -VOLUME_STEP), 0.0);
    }

    #[test]
    fn default_accelerators_parse() {
        for a in [
            "Ctrl+Alt+Space",
            "Ctrl+Alt+Right",
            "Ctrl+Alt+Left",
            "Ctrl+Alt+Up",
            "Ctrl+Alt+Down",
        ] {
            assert!(a.parse::<Shortcut>().is_ok(), "{a}");
        }
        assert!("Ctrl+Nonsense".parse::<Shortcut>().is_err());
    }
}
