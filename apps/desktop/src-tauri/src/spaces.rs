//! Local library spaces (T8.1). A space is a local data boundary, not an account or access
//! control mechanism. Its registry is device-wide; each entry points to an independently owned
//! library root and therefore to its own SQLite database, history and recommendations.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use tauri::{AppHandle, Manager, State};

use crate::{library::Library, playback::Playback, settings_store::write_atomic};

const FILE: &str = "spaces.json";

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Space {
    pub id: String,
    pub name: String,
    pub root: String,
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
    pub fn open(data_dir: &Path, legacy_root: Option<String>) -> Arc<Self> {
        let path = data_dir.join(FILE);
        let fallback_root = legacy_root.unwrap_or_else(|| {
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
            }],
        };
        let state = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .filter(|s: &SpacesState| {
                !s.spaces.is_empty() && s.spaces.iter().any(|x| x.id == s.active_id)
            })
            .unwrap_or(fallback);
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
    });
    let out = s.clone();
    drop(s);
    spaces.persist();
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
    let selected = spaces.find(&id)?;
    if let Some(playback) = app.try_state::<Playback>() {
        playback.stop_for_space_switch();
    }
    let library = app.state::<Library>();
    library.set_root(Path::new(&selected.root))?;
    spaces.activate(&selected.id)?;
    Ok(spaces.snapshot())
}
