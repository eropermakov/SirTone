//! Library bridge (Stage 1): library folder, background import, collection lists over IPC.
//! `library.db`, staging and quarantine live in `<root>\.sirtone\` (docs/03 §8.1). The import
//! thread owns its own connection (one writer); reads use a separate connection (WAL).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::ports::VolumeInfo;
use sirtone_library::Medium;
use sirtone_library::collection::{self, Counts, Cursor};
use sirtone_library::import::{self, ImportDeps, ImportEvent, SERVICE_DIR};
use sirtone_library::repo::{self, TitleCursor};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::settings_store::SettingsStore;

// ---------- IPC types ----------

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStatus {
    /// `None` until the user chose a folder (docs/03 §8.2).
    pub root: Option<String>,
    pub default_root: String,
    pub tracks: u32,
    pub artists: u32,
    pub albums: u32,
    pub unknown: u32,
    pub duration_sec: u32,
    /// Bytes as f64 (u64 is not representable in JS numbers).
    pub bytes: f64,
    pub importing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PageCursor {
    pub key: String,
    pub id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackItem {
    pub id: u32,
    pub title: String,
    pub artists: String,
    pub album: Option<String>,
    pub duration_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackPage {
    pub rows: Vec<TrackItem>,
    pub next: Option<PageCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AlbumItem {
    pub id: u32,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub tracks: u32,
    /// Cover hash: `http://sirtone-cover.localhost/<hash>/<size>`.
    pub cover: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AlbumPage {
    pub rows: Vec<AlbumItem>,
    pub next: Option<PageCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtistItem {
    pub id: u32,
    pub name: String,
    pub tracks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtistPage {
    pub rows: Vec<ArtistItem>,
    pub next: Option<PageCursor>,
}

/// Import progress for the sidebar line (DESIGN.md §9 Sidebar, Зейгарник). ~5 per second.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub done: u32,
    pub total: u32,
    /// All source files copied and verified: the medium can be removed (docs/05 §3.2).
    pub sources_released: bool,
}

/// Batch report (docs/05 §3.2): «Добавлено · Неизвестные · Дубли · Не добавлено».
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ImportFinished {
    pub added: u32,
    pub unknown: u32,
    pub duplicates: u32,
    pub rejected: u32,
    pub rejected_reasons: Vec<String>,
    pub cancelled: u32,
    pub can_eject: bool,
}

/// The collection changed (tracks added/removed): lists and counters refresh. Throttled.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct LibraryChanged {
    pub tracks: u32,
}

/// Files were dropped before a library folder was chosen: the UI asks for one.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRootNeeded {
    pub pending: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ImportStart {
    Started,
    RootNeeded,
}

// ---------- service ----------

/// A library track ready to play: absolute path, title, artists, album cover hash.
pub type Resolved = (PathBuf, String, String, Option<String>);

struct Shared {
    root: Option<PathBuf>,
    read: Option<Connection>,
    counts: Option<Counts>,
    pending: Vec<PathBuf>,
}

pub struct Library {
    shared: Arc<Mutex<Shared>>,
    settings: Arc<SettingsStore>,
    jobs: Sender<Vec<PathBuf>>,
    cancel: Arc<AtomicBool>,
    importing: Arc<AtomicBool>,
}

fn deps() -> ImportDeps {
    ImportDeps {
        probe: Arc::new(sirtone_audio::probe::SymphoniaProbe),
        tags: Arc::new(sirtone_platform::tags::WinTags::new(Arc::new(
            sirtone_recognize::LoftyTags,
        ))),
        identify: Arc::new(sirtone_recognize::TagIdentify),
        fs: Arc::new(sirtone_platform::files::WinFiles),
        volume: Arc::new(sirtone_platform::files::WinVolume),
        covers: Arc::new(sirtone_recognize::LoftyCovers),
    }
}

fn db_path(root: &Path) -> PathBuf {
    root.join(SERVICE_DIR).join("library.db")
}

fn open_db(root: &Path) -> Result<Connection, AppError> {
    let medium = if sirtone_platform::files::WinVolume.is_removable(root) {
        Medium::Removable
    } else {
        Medium::Fixed
    };
    sirtone_library::open(&db_path(root), medium).map_err(AppError::from)
}

/// `%USERPROFILE%\Music\SirTone` (docs/03 §8.2). Isolated test instances (`SIRTONE_DATA_DIR`)
/// get a folder inside their data dir, so pressing «Продолжить» never touches the owner's Music.
pub fn default_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("SIRTONE_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir).join("Music").join("SirTone");
    }
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Music")
        .join("SirTone")
}

fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Library {
    pub fn start(app: AppHandle, settings: Arc<SettingsStore>) -> Self {
        let root = settings.get()["library"]["root"]
            .as_str()
            .map(PathBuf::from)
            .filter(|p| p.is_absolute());
        let read = root.as_deref().and_then(|r| open_db(r).ok());
        let shared = Arc::new(Mutex::new(Shared {
            root,
            read,
            counts: None,
            pending: Vec::new(),
        }));
        let (jobs, rx) = mpsc::channel::<Vec<PathBuf>>();
        let cancel = Arc::new(AtomicBool::new(false));
        let importing = Arc::new(AtomicBool::new(false));
        let (w_shared, w_cancel, w_importing) = (shared.clone(), cancel.clone(), importing.clone());
        std::thread::Builder::new()
            .name("sirtone-import".into())
            .spawn(move || import_worker(app, w_shared, rx, w_cancel, w_importing))
            .ok();
        let lib = Self {
            shared,
            settings,
            jobs,
            cancel,
            importing,
        };
        // Recover the journal and resume interrupted batches right away (docs/05 §3.3).
        if lock(&lib.shared).root.is_some() {
            let _ = lib.jobs.send(Vec::new());
        }
        lib
    }

    pub fn status(&self) -> LibraryStatus {
        let mut s = lock(&self.shared);
        if s.counts.is_none()
            && let Some(c) = s.read.as_ref()
        {
            s.counts = collection::counts(c).ok();
        }
        let n = s.counts.clone().unwrap_or_default();
        LibraryStatus {
            root: s.root.as_ref().map(|p| p.to_string_lossy().into_owned()),
            default_root: default_root().to_string_lossy().into_owned(),
            tracks: n.tracks as u32,
            artists: n.artists as u32,
            albums: n.albums as u32,
            unknown: n.unknown as u32,
            duration_sec: (n.duration_ms / 1000) as u32,
            bytes: n.bytes as f64,
            importing: self.importing.load(Ordering::Relaxed),
        }
    }

    /// Validates and sets the library folder (creates it, checks it is writable), then starts any
    /// import that was waiting for it.
    pub fn set_root(&self, path: &Path) -> Result<(), AppError> {
        if !path.is_absolute() {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_details("not absolute"));
        }
        let service = path.join(SERVICE_DIR);
        std::fs::create_dir_all(&service)
            .map_err(|e| AppError::new(ErrorCode::PermissionDenied).with_details(e))?;
        // docs/03 §8.1: the service folder is hidden in Explorer.
        sirtone_platform::files::set_hidden(&service);
        let probe = service.join(".write-test");
        std::fs::write(&probe, b"ok")
            .map_err(|e| AppError::new(ErrorCode::PermissionDenied).with_details(e))?;
        let _ = std::fs::remove_file(&probe);
        let conn = open_db(path)?;
        self.settings
            .patch_durable(&serde_json::json!({ "library": { "root": path.to_string_lossy() } }))
            .map_err(|e| AppError::new(ErrorCode::InvalidConfig).with_details(e))?;
        let pending = {
            let mut s = lock(&self.shared);
            s.root = Some(path.to_path_buf());
            s.read = Some(conn);
            s.counts = None;
            std::mem::take(&mut s.pending)
        };
        tracing::info!(target: "sirtone::library", "library folder set");
        let _ = self.jobs.send(pending);
        Ok(())
    }

    /// Drop / «Добавить папку»: imports into the library, or parks the paths until a folder is
    /// chosen (FR-IMP-001).
    pub fn import_paths(&self, app: &AppHandle, paths: Vec<PathBuf>) -> ImportStart {
        let mut s = lock(&self.shared);
        if s.root.is_none() {
            s.pending.extend(paths);
            let _ = LibraryRootNeeded {
                pending: s.pending.len() as u32,
            }
            .emit(app);
            return ImportStart::RootNeeded;
        }
        drop(s);
        self.cancel.store(false, Ordering::Relaxed);
        let _ = self.jobs.send(paths);
        ImportStart::Started
    }

    fn with_read<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, sirtone_library::LibraryError>,
    ) -> Result<T, AppError> {
        let s = lock(&self.shared);
        let c = s
            .read
            .as_ref()
            .ok_or_else(|| AppError::new(ErrorCode::LibraryRootNotSet))?;
        f(c).map_err(AppError::from)
    }

    /// Serves `sirtone-cover://<hash>/<size>` (docs/03 §5.6): WebP bytes, generated on first use.
    pub fn cover_bytes(&self, hash: &str, size: u32) -> Option<Vec<u8>> {
        let root = lock(&self.shared).root.clone()?;
        let path = sirtone_library::covers::variant(&root, hash, size).ok()?;
        std::fs::read(path).ok()
    }

    /// Absolute paths + display data + album cover hash for track ids (in that order).
    pub fn resolve(&self, ids: &[u32]) -> Result<Vec<Resolved>, AppError> {
        let root = lock(&self.shared)
            .root
            .clone()
            .ok_or_else(|| AppError::new(ErrorCode::LibraryRootNotSet))?;
        self.with_read(|c| {
            let mut st = c.prepare_cached(
                "SELECT t.rel_path, t.title,
                        coalesce((SELECT group_concat(name, ', ') FROM
                          (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                           WHERE ta.track_id = t.id ORDER BY ta.position)), ''),
                        (SELECT cv.hash FROM album al JOIN cover cv ON cv.id = al.cover_id WHERE al.id = t.album_id)
                 FROM track t WHERE t.id = ?1",
            )?;
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Ok((rel, title, artists, cover)) = st.query_row([id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                }) {
                    out.push((rel.split('/').fold(root.clone(), |p, c| p.join(c)), title, artists, cover));
                }
            }
            Ok(out)
        })
    }
}

fn import_worker(
    app: AppHandle,
    shared: Arc<Mutex<Shared>>,
    rx: mpsc::Receiver<Vec<PathBuf>>,
    cancel: Arc<AtomicBool>,
    importing: Arc<AtomicBool>,
) {
    let deps = deps();
    let mut conn: Option<(PathBuf, Connection)> = None;
    while let Ok(paths) = rx.recv() {
        let Some(root) = lock(&shared).root.clone() else {
            continue;
        };
        if conn.as_ref().is_none_or(|(r, _)| *r != root) {
            conn = open_db(&root).ok().map(|c| (root.clone(), c));
        }
        let Some((_, db)) = conn.as_mut() else {
            continue;
        };
        importing.store(true, Ordering::Relaxed);
        if let Err(e) = import::recover(db, &root, deps.fs.as_ref()) {
            tracing::error!(target: "sirtone::import", error = %e, "recovery failed");
        }
        // Covers for albums imported before T1.26 (or without art on their first track).
        if let Ok(n) = sirtone_library::covers::backfill(db, &root, deps.covers.as_ref(), 500)
            && n > 0
        {
            tracing::info!(target: "sirtone::import", albums = n, "covers backfilled");
            emit_changed(&app, &shared);
        }
        let mut batches = import::unfinished_batches(db).unwrap_or_default();
        if !paths.is_empty() {
            match import::start_batch(db, &paths, "drop") {
                Ok(b) => batches.push(b),
                Err(e) => tracing::error!(target: "sirtone::import", error = %e, "start failed"),
            }
        }
        for batch in batches {
            let mut last_progress = Instant::now() - Duration::from_secs(1);
            let mut last_change = Instant::now();
            let mut added_since = 0u32;
            let mut released = false;
            let shared_ev = shared.clone();
            let app_ev = app.clone();
            let mut on_event = |e: ImportEvent| match e {
                ImportEvent::Progress { done, total } => {
                    if done == total || last_progress.elapsed() >= Duration::from_millis(200) {
                        last_progress = Instant::now();
                        let _ = ImportProgress {
                            done: done as u32,
                            total: total as u32,
                            sources_released: released,
                        }
                        .emit(&app_ev);
                    }
                }
                ImportEvent::TrackAdded { .. } => {
                    added_since += 1;
                    if last_change.elapsed() >= Duration::from_millis(400) {
                        last_change = Instant::now();
                        emit_changed(&app_ev, &shared_ev);
                        added_since = 0;
                    }
                }
                ImportEvent::SourcesReleased => {
                    released = true;
                }
            };
            match import::run_batch(db, &root, batch, &deps, &cancel, &mut on_event) {
                Ok(r) => {
                    let mut reasons: Vec<String> =
                        r.rejected.iter().map(|(_, r)| r.clone()).collect();
                    reasons.sort();
                    reasons.dedup();
                    let _ = ImportFinished {
                        added: r.added as u32,
                        unknown: r.unknown as u32,
                        duplicates: r.duplicates.len() as u32,
                        rejected: r.rejected.len() as u32,
                        rejected_reasons: reasons,
                        cancelled: r.cancelled as u32,
                        can_eject: r.can_eject,
                    }
                    .emit(&app);
                    tracing::info!(target: "sirtone::import", added = r.added, unknown = r.unknown, "batch done");
                }
                Err(e) => tracing::error!(target: "sirtone::import", error = %e, "batch failed"),
            }
            let _ = added_since;
            emit_changed(&app, &shared);
        }
        cancel.store(false, Ordering::Relaxed);
        importing.store(false, Ordering::Relaxed);
    }
}

fn emit_changed(app: &AppHandle, shared: &Mutex<Shared>) {
    let mut s = lock(shared);
    s.counts = s.read.as_ref().and_then(|c| collection::counts(c).ok());
    let tracks = s.counts.as_ref().map_or(0, |c| c.tracks as u32);
    drop(s);
    let _ = LibraryChanged { tracks }.emit(app);
}

// ---------- commands ----------

fn track_item(r: repo::TrackRow) -> TrackItem {
    TrackItem {
        id: r.id as u32,
        title: r.title,
        artists: r.artists,
        album: r.album,
        duration_ms: u32::try_from(r.duration_ms).unwrap_or(u32::MAX),
    }
}

#[tauri::command]
#[specta::specta]
pub fn library_status(lib: State<'_, Library>) -> LibraryStatus {
    lib.status()
}

/// Folder picker (native dialog); `None` when the user cancels.
#[tauri::command]
#[specta::specta]
pub async fn library_pick_folder(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
#[specta::specta]
pub fn library_set_root(lib: State<'_, Library>, path: String) -> Result<LibraryStatus, AppError> {
    lib.set_root(Path::new(&path))?;
    Ok(lib.status())
}

#[tauri::command]
#[specta::specta]
pub fn import_paths(app: AppHandle, lib: State<'_, Library>, paths: Vec<String>) -> ImportStart {
    lib.import_paths(&app, paths.into_iter().map(PathBuf::from).collect())
}

#[tauri::command]
#[specta::specta]
pub fn import_cancel(lib: State<'_, Library>) {
    lib.cancel.store(true, Ordering::Relaxed);
}

#[tauri::command]
#[specta::specta]
pub fn collection_tracks(
    lib: State<'_, Library>,
    after: Option<PageCursor>,
    limit: u32,
) -> Result<TrackPage, AppError> {
    let cur = after.map(|c| TitleCursor {
        title_norm: c.key,
        id: i64::from(c.id),
    });
    lib.with_read(|c| {
        let (rows, next) = repo::tracks_by_title(c, cur.as_ref(), limit.clamp(1, 500))?;
        Ok(TrackPage {
            rows: rows.into_iter().map(track_item).collect(),
            next: next.map(|n| PageCursor {
                key: n.title_norm,
                id: n.id as u32,
            }),
        })
    })
}

#[tauri::command]
#[specta::specta]
pub fn collection_albums(
    lib: State<'_, Library>,
    after: Option<PageCursor>,
    limit: u32,
) -> Result<AlbumPage, AppError> {
    let cur = after.map(|c| Cursor {
        key: c.key,
        id: i64::from(c.id),
    });
    lib.with_read(|c| {
        let (rows, next) = collection::albums_page(c, cur.as_ref(), limit.clamp(1, 500))?;
        Ok(AlbumPage {
            rows: rows
                .into_iter()
                .map(|a| AlbumItem {
                    id: a.id as u32,
                    title: a.title,
                    artist: a.artist,
                    year: a.year,
                    tracks: a.tracks,
                    cover: a.cover,
                })
                .collect(),
            next: next.map(|n| PageCursor {
                key: n.key,
                id: n.id as u32,
            }),
        })
    })
}

#[tauri::command]
#[specta::specta]
pub fn collection_artists(
    lib: State<'_, Library>,
    after: Option<PageCursor>,
    limit: u32,
) -> Result<ArtistPage, AppError> {
    let cur = after.map(|c| Cursor {
        key: c.key,
        id: i64::from(c.id),
    });
    lib.with_read(|c| {
        let (rows, next) = collection::artists_page(c, cur.as_ref(), limit.clamp(1, 500))?;
        Ok(ArtistPage {
            rows: rows
                .into_iter()
                .map(|a| ArtistItem {
                    id: a.id as u32,
                    name: a.name,
                    tracks: a.tracks,
                })
                .collect(),
            next: next.map(|n| PageCursor {
                key: n.key,
                id: n.id as u32,
            }),
        })
    })
}

#[tauri::command]
#[specta::specta]
pub fn unknown_tracks(lib: State<'_, Library>) -> Result<Vec<TrackItem>, AppError> {
    lib.with_read(|c| {
        Ok(collection::unknown_tracks(c, 5000)?
            .into_iter()
            .map(track_item)
            .collect())
    })
}

#[tauri::command]
#[specta::specta]
pub fn album_tracks(lib: State<'_, Library>, album: u32) -> Result<Vec<TrackItem>, AppError> {
    lib.with_read(|c| {
        Ok(collection::album_tracks(c, i64::from(album))?
            .into_iter()
            .map(track_item)
            .collect())
    })
}

#[tauri::command]
#[specta::specta]
pub fn artist_tracks(lib: State<'_, Library>, artist: u32) -> Result<Vec<TrackItem>, AppError> {
    lib.with_read(|c| {
        Ok(collection::artist_tracks(c, i64::from(artist))?
            .into_iter()
            .map(track_item)
            .collect())
    })
}

/// `http://sirtone-cover.localhost/<hash>/<size>` handler: only known hashes and sizes resolve to
/// files of the cover cache; everything else is 404 (docs/03 §5.6).
pub fn cover_protocol(app: &AppHandle, path: &str) -> tauri::http::Response<Vec<u8>> {
    use tauri::Manager;
    let mut parts = path.trim_matches('/').split('/');
    let found = match (
        parts.next(),
        parts.next().and_then(|s| s.parse::<u32>().ok()),
        parts.next(),
    ) {
        (Some(hash), Some(size), None) => app
            .try_state::<Library>()
            .and_then(|l| l.cover_bytes(hash, size)),
        _ => None,
    };
    match found {
        Some(bytes) => tauri::http::Response::builder()
            .status(200)
            .header("Content-Type", "image/webp")
            .header("Cache-Control", "public, max-age=31536000, immutable")
            .body(bytes),
        None => tauri::http::Response::builder()
            .status(404)
            .body(Vec::new()),
    }
    .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}
