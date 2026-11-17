//! Local spaces (D-178, FR-DATA-006).
//!
//! A space is deliberately not an account or a profile.  Its complete application-data folder is
//! separate, so the library database, preference document, WebView profile and learning history
//! cannot be accidentally shared.  Changing the active space restarts the process instead of
//! attempting to replace live SQLite/audio/watcher services.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use tauri::{AppHandle, State};

const FILE: &str = "spaces.json";
const DEFAULT_ID: &str = "default";
const DEFAULT_NAME: &str = "Моя музыка";
const MAX_NAME_LEN: usize = 40;

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LocalSpace {
    pub id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedSpace {
    id: String,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Registry {
    spaces: Vec<SavedSpace>,
}

pub struct Spaces {
    root: PathBuf,
    registry: Mutex<Registry>,
    active: String,
}

impl Spaces {
    pub fn open() -> Arc<Self> {
        let root = base_data_dir();
        let active = std::env::var("SIRTONE_SPACE_ID").ok();
        Self::open_at(root, active.as_deref())
    }

    fn open_at(root: PathBuf, requested_active: Option<&str>) -> Arc<Self> {
        let mut registry = load(&root.join(FILE));
        if registry.spaces.is_empty() {
            registry.spaces.push(SavedSpace {
                id: DEFAULT_ID.to_owned(),
                name: DEFAULT_NAME.to_owned(),
            });
        }
        let active = requested_active
            .filter(|id| registry.spaces.iter().any(|space| space.id == *id))
            .map(str::to_owned)
            .unwrap_or_else(|| DEFAULT_ID.to_owned());
        let result = Arc::new(Self {
            root,
            registry: Mutex::new(registry),
            active,
        });
        let _ = result.save();
        result
    }

    fn path(&self) -> PathBuf {
        self.root.join(FILE)
    }

    fn save(&self) -> std::io::Result<()> {
        let guard = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let text = serde_json::to_string_pretty(&*guard).map_err(std::io::Error::other)?;
        if let Some(parent) = self.path().parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = self.path().with_extension("json.tmp");
        fs::write(&temp, text)?;
        fs::rename(temp, self.path())
    }

    fn list(&self) -> Vec<LocalSpace> {
        self.registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .spaces
            .iter()
            .map(|space| LocalSpace {
                id: space.id.clone(),
                name: space.name.clone(),
                active: space.id == self.active,
            })
            .collect()
    }

    fn create(&self, name: String) -> Result<LocalSpace, AppError> {
        let name = checked_name(name)?;
        let id = uuid::Uuid::now_v7().to_string();
        {
            let mut registry = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            registry.spaces.push(SavedSpace {
                id: id.clone(),
                name: name.clone(),
            });
        }
        self.save().map_err(io_error)?;
        Ok(LocalSpace {
            id,
            name,
            active: false,
        })
    }

    fn rename(&self, id: &str, name: String) -> Result<(), AppError> {
        let name = checked_name(name)?;
        let mut registry = self.registry.lock().unwrap_or_else(|p| p.into_inner());
        let Some(space) = registry.spaces.iter_mut().find(|space| space.id == id) else {
            return Err(not_found());
        };
        space.name = name;
        drop(registry);
        self.save().map_err(io_error)
    }

    fn remove(&self, id: &str) -> Result<(), AppError> {
        if id == self.active || id == DEFAULT_ID {
            return Err(AppError::new(ErrorCode::InvalidArgument));
        }
        let data = space_dir(&self.root, id).ok_or_else(not_found)?;
        {
            let registry = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            if !registry.spaces.iter().any(|space| space.id == id) {
                return Err(not_found());
            }
        }
        // The OS recycle bin keeps a recoverable copy of every local database and track. Do this
        // before changing the registry: a failed deletion must leave the space selectable.
        if data.exists() {
            trash::delete(&data).map_err(|_| AppError::new(ErrorCode::PermissionDenied))?;
        }
        {
            let mut registry = self.registry.lock().unwrap_or_else(|p| p.into_inner());
            registry.spaces.retain(|space| space.id != id);
        }
        self.save().map_err(io_error)
    }

    fn select(&self, app: &AppHandle, id: &str) -> Result<(), AppError> {
        if !self
            .registry
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .spaces
            .iter()
            .any(|space| space.id == id)
        {
            return Err(not_found());
        }
        if id == self.active {
            return Ok(());
        }
        // SAFETY: this only changes the current process environment immediately before Tauri
        // creates its replacement process.  No worker reads these values after startup.
        unsafe {
            if id == DEFAULT_ID {
                std::env::remove_var("SIRTONE_DATA_DIR");
                std::env::remove_var("SIRTONE_SPACE_ID");
            } else {
                let dir = space_dir(&self.root, id).ok_or_else(not_found)?;
                std::env::set_var("SIRTONE_DATA_DIR", dir);
                std::env::set_var("SIRTONE_SPACE_ID", id);
            }
        }
        app.restart()
    }
}

fn checked_name(raw: String) -> Result<String, AppError> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_LEN || name.contains(['/', '\\', '\0']) {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    Ok(name.to_owned())
}

fn load(path: &Path) -> Registry {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Registry { spaces: vec![] })
}

fn base_data_dir() -> PathBuf {
    if let Some(dir) = crate::portable::data_dir() {
        return dir;
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("SirTone")
}

fn space_dir(root: &Path, id: &str) -> Option<PathBuf> {
    let parsed = uuid::Uuid::parse_str(id).ok()?;
    Some(root.join("spaces").join(parsed.to_string()))
}

fn not_found() -> AppError {
    AppError::new(ErrorCode::InvalidArgument)
}

fn io_error(_: std::io::Error) -> AppError {
    AppError::new(ErrorCode::PermissionDenied)
}

#[tauri::command]
#[specta::specta]
pub fn spaces_list(spaces: State<'_, Arc<Spaces>>) -> Vec<LocalSpace> {
    spaces.list()
}

#[tauri::command]
#[specta::specta]
pub fn spaces_create(spaces: State<'_, Arc<Spaces>>, name: String) -> Result<LocalSpace, AppError> {
    spaces.create(name)
}

#[tauri::command]
#[specta::specta]
pub fn spaces_rename(
    spaces: State<'_, Arc<Spaces>>,
    id: String,
    name: String,
) -> Result<(), AppError> {
    spaces.rename(&id, name)
}

#[tauri::command]
#[specta::specta]
pub fn spaces_remove(spaces: State<'_, Arc<Spaces>>, id: String) -> Result<(), AppError> {
    spaces.remove(&id)
}

#[tauri::command]
#[specta::specta]
pub fn spaces_select(
    app: AppHandle,
    spaces: State<'_, Arc<Spaces>>,
    id: String,
) -> Result<(), AppError> {
    spaces.select(&app, &id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names_without_file_path_characters() {
        assert_eq!(checked_name("  Друг  ".into()).unwrap(), "Друг");
        assert!(checked_name("".into()).is_err());
        assert!(checked_name("a/b".into()).is_err());
    }

    #[test]
    fn only_uuid_ids_can_address_a_space_directory() {
        assert!(space_dir(Path::new("C:/x"), "..\\outside").is_none());
        assert!(space_dir(Path::new("C:/x"), &uuid::Uuid::now_v7().to_string()).is_some());
    }

    #[test]
    fn creates_and_renames_a_space_in_an_isolated_registry() {
        let root = std::env::temp_dir().join(format!("sirtone-spaces-{}", uuid::Uuid::now_v7()));
        let spaces = Spaces::open_at(root.clone(), None);
        let created = spaces.create("Для друга".into()).unwrap();
        spaces.rename(&created.id, "Гость".into()).unwrap();
        let list = spaces.list();
        assert_eq!(list.len(), 2);
        assert!(
            list.iter()
                .any(|space| space.id == created.id && space.name == "Гость")
        );
        assert!(root.join(FILE).is_file());
        let _ = std::fs::remove_dir_all(root);
    }
}
