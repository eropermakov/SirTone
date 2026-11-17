//! Local library spaces (T8.1). A space is a local data boundary, not an account or access
//! control mechanism. Its registry is device-wide; each entry points to an independently owned
//! library root and therefore to its own SQLite database, history and recommendations.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    library::Library,
    playback::Playback,
    settings_store::{SettingsStore, write_atomic},
};

const FILE: &str = "spaces.json";

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    pub id: String,
    pub name: String,
    pub root: String,
    #[serde(default = "empty_preferences")]
    #[specta(skip)]
    pub preferences: serde_json::Value,
}

fn empty_preferences() -> serde_json::Value {
    serde_json::Value::Object(Default::default())
}

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpacesState {
    pub active_id: String,
    pub spaces: Vec<Space>,
}

pub struct Spaces {
    path: PathBuf,
    state: Mutex<SpacesState>,
}

impl Spaces {
    pub fn open(data_dir: &Path, settings: &SettingsStore) -> Arc<Self> {
        let path = data_dir.join(FILE);
        let fallback_root = settings.get()["library"]["root"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                crate::library::default_root()
                    .to_string_lossy()
                    .into_owned()
            });
        let fallback = SpacesState {
            active_id: "default".into(),
            spaces: vec![Space {
                id: "default".into(),
                name: "Моё пространство".into(),
                root: fallback_root,
                preferences: settings.space_preferences(),
            }],
        };
        let mut state = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .filter(|s: &SpacesState| {
                !s.spaces.is_empty() && s.spaces.iter().any(|x| x.id == s.active_id)
            })
            .unwrap_or(fallback);
        // Registry versions before per-space preferences inherit the active legacy settings
        // exactly once. Other spaces start from defaults when created.
        for space in &mut state.spaces {
            if space.preferences.as_object().is_some_and(|p| p.is_empty()) {
                space.preferences = if space.id == state.active_id {
                    settings.space_preferences()
                } else {
                    SettingsStore::default_space_preferences()
                };
            }
        }
        let this = Arc::new(Self {
            path,
            state: Mutex::new(state),
        });
        this.persist();
        this
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, SpacesState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn persist(&self) {
        let snapshot = self.guard().clone();
        let _ = write_atomic(
            &self.path,
            &serde_json::to_value(snapshot).unwrap_or_default(),
        );
    }

    pub fn snapshot(&self) -> SpacesState {
        self.guard().clone()
    }

    fn find(&self, id: &str) -> Result<Space, AppError> {
        self.guard()
            .spaces
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument).with_details("unknown space"))
    }

    fn activate(&self, id: &str) -> Result<(), AppError> {
        let mut state = self.guard();
        if !state.spaces.iter().any(|s| s.id == id) {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_details("unknown space"));
        }
        state.active_id = id.to_owned();
        drop(state);
        self.persist();
        Ok(())
    }

    fn save_active_preferences(&self, preferences: serde_json::Value) {
        let mut state = self.guard();
        let active_id = state.active_id.clone();
        if let Some(active) = state.spaces.iter_mut().find(|space| space.id == active_id) {
            active.preferences = preferences;
        }
        drop(state);
        self.persist();
    }
}

#[tauri::command]
#[specta::specta]
pub fn spaces_list(spaces: State<'_, Arc<Spaces>>) -> SpacesState {
    spaces.snapshot()
}

#[tauri::command]
#[specta::specta]
pub fn spaces_create(
    spaces: State<'_, Arc<Spaces>>,
    name: String,
    root: String,
) -> Result<SpacesState, AppError> {
    let root = PathBuf::from(root);
    if name.trim().is_empty() || !root.is_absolute() {
        return Err(AppError::new(ErrorCode::InvalidArgument).with_details("name or root"));
    }
    let mut s = spaces.guard();
    if s.spaces
        .iter()
        .any(|x| x.root.eq_ignore_ascii_case(&root.to_string_lossy()))
    {
        return Err(AppError::new(ErrorCode::InvalidArgument)
            .with_details("root already belongs to a space"));
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let id = format!("space-{stamp}-{}", s.spaces.len());
    s.spaces.push(Space {
        id,
        name: name.trim().to_owned(),
        root: root.to_string_lossy().into_owned(),
        preferences: SettingsStore::default_space_preferences(),
    });
    let out = s.clone();
    drop(s);
    spaces.persist();
    Ok(out)
}

#[tauri::command]
#[specta::specta]
pub fn spaces_rename(
    spaces: State<'_, Arc<Spaces>>,
    id: String,
    name: String,
) -> Result<SpacesState, AppError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidArgument).with_details("empty space name"));
    }
    let mut state = spaces.guard();
    let space = state
        .spaces
        .iter_mut()
        .find(|space| space.id == id)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument).with_details("unknown space"))?;
    space.name = name.to_owned();
    let out = state.clone();
    drop(state);
    spaces.persist();
    Ok(out)
}

/// Removes only the local registry entry. Music and `.sirtone` data are deliberately untouched.
/// An active entry is first switched to a remaining one, so readers never observe no space.
#[tauri::command]
#[specta::specta]
pub fn spaces_delete(
    app: AppHandle,
    spaces: State<'_, Arc<Spaces>>,
    id: String,
) -> Result<SpacesState, AppError> {
    let next = {
        let state = spaces.guard();
        if state.spaces.len() < 2 {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_details("last space"));
        }
        if !state.spaces.iter().any(|space| space.id == id) {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_details("unknown space"));
        }
        (state.active_id == id)
            .then(|| {
                state
                    .spaces
                    .iter()
                    .find(|space| space.id != id)
                    .cloned()
                    .ok_or_else(|| {
                        AppError::new(ErrorCode::Internal).with_details("no replacement space")
                    })
            })
            .transpose()?
    };
    let settings = app.state::<Arc<SettingsStore>>();
    spaces.save_active_preferences(settings.space_preferences());
    let changed = if let Some(next) = &next {
        settings
            .apply_space_preferences(&next.preferences)
            .map_err(|e| AppError::new(ErrorCode::InvalidConfig).with_details(e))?
    } else {
        Vec::new()
    };
    if let Some(next) = &next {
        if let Some(playback) = app.try_state::<Playback>() {
            playback.stop_for_space_switch();
        }
        app.state::<Library>().set_root(Path::new(&next.root))?;
    }
    let mut state = spaces.guard();
    state.spaces.retain(|space| space.id != id);
    if let Some(next) = &next {
        state.active_id = next.id.clone();
    }
    let out = state.clone();
    drop(state);
    spaces.persist();
    let _ = app.emit("settings://changed", &changed);
    Ok(out)
}

/// Switches the fully isolated library service. Playback is stopped by the caller before this
/// command is exposed in the UI; this command never mutates audio source files.
#[tauri::command]
#[specta::specta]
pub fn spaces_select(
    app: AppHandle,
    spaces: State<'_, Arc<Spaces>>,
    id: String,
) -> Result<SpacesState, AppError> {
    let settings = app.state::<Arc<SettingsStore>>();
    spaces.save_active_preferences(settings.space_preferences());
    let selected = spaces.find(&id)?;
    let changed = settings
        .apply_space_preferences(&selected.preferences)
        .map_err(|e| AppError::new(ErrorCode::InvalidConfig).with_details(e))?;
    if let Some(playback) = app.try_state::<Playback>() {
        playback.stop_for_space_switch();
    }
    let library = app.state::<Library>();
    library.set_root(Path::new(&selected.root))?;
    spaces.activate(&selected.id)?;
    let _ = app.emit("settings://changed", &changed);
    Ok(spaces.snapshot())
}
