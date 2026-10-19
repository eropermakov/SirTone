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
use sirtone_library::reconcile::{Outcome, Report};
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
    /// The folder is set but cannot be reached (stick pulled, renamed, another library there):
    /// nothing is created, marked or deleted until it is back (FR-FS-004).
    pub offline: bool,
    /// Many files vanished at once: how many, waiting for the user (FR-FS-003).
    pub gone_question: Option<u32>,
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
    pub album_id: Option<u32>,
    pub artist_id: Option<u32>,
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
    /// Newest track, ms (sort «Недавно добавленные»).
    pub added_at: f64,
    /// Placeholder hint when there is no cover: `genre << 4 | mood` (T3.20, `art.rs`).
    pub art: u8,
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
    /// Newest track, ms (sort «Недавно добавленные»).
    pub added_at: f64,
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
    /// For `import_report` (T3.21).
    pub batch: u32,
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

/// Files added in Explorer lie elsewhere than the layout rules say (T3.19, FR-FS-005): the UI
/// offers «Навести порядок» (setting `library.organize_external` = `suggest`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeSuggested {
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

/// Work for the import thread: imports and reconcile passes never run at the same time, so the
/// pipeline's own placements can never race the watcher (docs/05 §7).
enum Job {
    Import(Vec<PathBuf>),
    Reconcile {
        confirmed: bool,
    },
    /// Manual assignment from «Неизвестные» / the track card: move + row, then reply.
    Assign {
        track: i64,
        assignment: sirtone_library::relocate::Assignment,
        /// Confirmed by MusicBrainz (D-87): recording id, confidence and the release group,
        /// stamped after the move.
        online: Option<(String, f64, Option<String>)>,
        reply: mpsc::Sender<Result<String, AppError>>,
    },
    /// FR-DATA-001: remove a track (file to the Recycle Bin, or only from the app).
    Remove {
        track: i64,
        with_file: bool,
        reply: mpsc::Sender<Result<String, AppError>>,
    },
    /// T3.12: merge two artists (database), then put the files in place track by track.
    MergeArtists {
        keep: i64,
        drop: i64,
        reply: mpsc::Sender<Result<(sirtone_library::artists::MergeRecord, u32), AppError>>,
    },
    /// T3.19: «Навести порядок» — the adopted files waiting for it, or the whole library.
    Organize {
        all: bool,
        reply: mpsc::Sender<Result<u32, AppError>>,
    },
    /// T3.12: undo a merge of this session, then put the files back.
    UndoMerge {
        record: Box<sirtone_library::artists::MergeRecord>,
        reply: mpsc::Sender<Result<u32, AppError>>,
    },
}

/// A library track ready to play: absolute path, title, artists, album cover hash.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub id: u32,
    pub path: PathBuf,
    pub title: String,
    pub artists: String,
    pub cover: Option<String>,
    pub album_id: Option<i64>,
    /// Primary artist (shuffle spreading, «К исполнителю»).
    pub artist_id: Option<i64>,
    /// Placeholder hint (T3.20).
    pub art: u8,
}

struct Shared {
    root: Option<PathBuf>,
    read: Option<Connection>,
    counts: Option<Counts>,
    pending: Vec<PathBuf>,
    offline: bool,
    gone_question: Option<u32>,
    watch: Option<sirtone_library::watch::Watch>,
    /// Adopted tracks out of place, waiting for «Навести порядок» (T3.19).
    organize: Vec<i64>,
}

pub struct Library {
    shared: Arc<Mutex<Shared>>,
    settings: Arc<SettingsStore>,
    jobs: Sender<Job>,
    cancel: Arc<AtomicBool>,
    importing: Arc<AtomicBool>,
    /// Short user edits (playlists): their own connection, so a write waiting on the import
    /// worker never holds `shared` (lists keep answering in ≤ 100 ms).
    writer: Mutex<Option<(PathBuf, Connection)>>,
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

pub(crate) fn db_path(root: &Path) -> PathBuf {
    root.join(SERVICE_DIR).join("library.db")
}

pub(crate) fn medium(root: &Path) -> Medium {
    if sirtone_platform::files::WinVolume.is_removable(root) {
        Medium::Removable
    } else {
        Medium::Fixed
    }
}

fn open_db(root: &Path) -> Result<Connection, AppError> {
    sirtone_library::open(&db_path(root), medium(root)).map_err(AppError::from)
}

/// Opens the library at `root` only if it is there and is the one we set up (`library_uid` in
/// settings). Never creates anything: a missing or foreign library is offline.
fn open_known(root: &Path, uid: Option<&str>) -> Option<Connection> {
    if !root.is_dir() {
        return None;
    }
    let conn = sirtone_library::open_existing(&db_path(root), medium(root)).ok()?;
    let actual = sirtone_library::db::meta(&conn, "library_uid")
        .ok()
        .flatten();
    match (uid, actual.as_deref()) {
        (Some(want), Some(have)) if want != have => None,
        _ => Some(conn),
    }
}

fn settings_uid(settings: &SettingsStore) -> Option<String> {
    settings.get()["library"]["uid"].as_str().map(str::to_owned)
}

/// Remembers which library lives at the root (first run after an update adopts the existing one).
fn remember_uid(settings: &SettingsStore, conn: &Connection) {
    if let Ok(Some(uid)) = sirtone_library::db::meta(conn, "library_uid")
        && settings_uid(settings).as_deref() != Some(uid.as_str())
    {
        let _ = settings.patch_durable(&serde_json::json!({ "library": { "uid": uid } }));
    }
}

/// Arms the watcher: every settled change queues a reconcile pass.
fn arm_watch(root: &Path, jobs: &Sender<Job>) -> Option<sirtone_library::watch::Watch> {
    let jobs = jobs.clone();
    match sirtone_library::watch::watch(root, move || {
        let _ = jobs.send(Job::Reconcile { confirmed: false });
    }) {
        Ok(w) => Some(w),
        Err(e) => {
            tracing::warn!(target: "sirtone::library", error = %e, "watcher unavailable");
            None
        }
    }
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

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Library {
    pub fn start(app: AppHandle, settings: Arc<SettingsStore>) -> Self {
        let root = settings.get()["library"]["root"]
            .as_str()
            .map(PathBuf::from)
            .filter(|p| p.is_absolute());
        let uid = settings_uid(&settings);
        let read = root.as_deref().and_then(|r| open_known(r, uid.as_deref()));
        if let Some(c) = read.as_ref() {
            remember_uid(&settings, c);
        }
        let offline = root.is_some() && read.is_none();
        if offline {
            tracing::warn!(target: "sirtone::library", "library folder unavailable — offline");
        }
        let (jobs, rx) = mpsc::channel::<Job>();
        let watch = match (&root, &read) {
            (Some(r), Some(_)) => arm_watch(r, &jobs),
            _ => None,
        };
        let shared = Arc::new(Mutex::new(Shared {
            root,
            read,
            counts: None,
            pending: Vec::new(),
            offline,
            gone_question: None,
            watch,
            organize: Vec::new(),
        }));
        let cancel = Arc::new(AtomicBool::new(false));
        let importing = Arc::new(AtomicBool::new(false));
        let (w_shared, w_cancel, w_importing) = (shared.clone(), cancel.clone(), importing.clone());
        let (m_app, m_shared, m_settings, m_jobs) =
            (app.clone(), shared.clone(), settings.clone(), jobs.clone());
        std::thread::Builder::new()
            .name("sirtone-import".into())
            .spawn(move || import_worker(app, w_shared, rx, w_cancel, w_importing))
            .ok();
        std::thread::Builder::new()
            .name("sirtone-root-monitor".into())
            .spawn(move || root_monitor(m_app, m_shared, m_settings, m_jobs))
            .ok();
        let lib = Self {
            shared,
            settings,
            jobs,
            cancel,
            importing,
            writer: Mutex::new(None),
        };
        // Recover the journal, resume interrupted batches and reconcile with the disk right away
        // (docs/05 §3.3, §7).
        if lock(&lib.shared).read.is_some() {
            let _ = lib.jobs.send(Job::Reconcile { confirmed: false });
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
            offline: s.offline,
            gone_question: s.gone_question,
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
        // The only place a library is created: an explicit choice of folder. A folder that
        // already holds one («Указать новое место») keeps it and its uid.
        let conn = open_db(path)?;
        let uid = sirtone_library::db::meta(&conn, "library_uid")
            .ok()
            .flatten()
            .unwrap_or_default();
        self.settings
            .patch_durable(&serde_json::json!({
                "library": { "root": path.to_string_lossy(), "uid": uid }
            }))
            .map_err(|e| AppError::new(ErrorCode::InvalidConfig).with_details(e))?;
        let watch = arm_watch(path, &self.jobs);
        let pending = {
            let mut s = lock(&self.shared);
            s.root = Some(path.to_path_buf());
            s.read = Some(conn);
            s.counts = None;
            s.offline = false;
            s.gone_question = None;
            s.watch = watch;
            std::mem::take(&mut s.pending)
        };
        tracing::info!(target: "sirtone::library", "library folder set");
        let _ = self.jobs.send(Job::Reconcile { confirmed: false });
        if !pending.is_empty() {
            let _ = self.jobs.send(Job::Import(pending));
        }
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
        let _ = self.jobs.send(Job::Import(paths));
        ImportStart::Started
    }

    /// Runs a short write on the writer connection of the current (online) library. A failed
    /// attempt reopens the connection and tries once more (a stick pulled and put back leaves a
    /// dead handle; each write is one transaction, so a failed one changed nothing).
    pub(crate) fn with_write<T>(
        &self,
        mut f: impl FnMut(&mut Connection) -> Result<T, sirtone_library::LibraryError>,
    ) -> Result<T, AppError> {
        let root = {
            let s = lock(&self.shared);
            match (&s.root, &s.read) {
                (Some(root), Some(_)) => root.clone(),
                _ => return Err(AppError::new(ErrorCode::LibraryRootNotSet)),
            }
        };
        let mut w = lock(&self.writer);
        let mut last = None;
        for _ in 0..2 {
            if w.as_ref().is_none_or(|(r, _)| *r != root) {
                let conn = sirtone_library::open_existing(&db_path(&root), medium(&root))?;
                *w = Some((root.clone(), conn));
            }
            let Some((_, conn)) = w.as_mut() else {
                return Err(AppError::new(ErrorCode::Internal));
            };
            match f(conn) {
                Ok(v) => return Ok(v),
                Err(e) => {
                    *w = None;
                    last = Some(e);
                }
            }
        }
        Err(last.map_or_else(|| AppError::new(ErrorCode::Internal), AppError::from))
    }

    /// The configured library folder.
    pub(crate) fn root(&self) -> Option<PathBuf> {
        lock(&self.shared).root.clone()
    }

    pub(crate) fn with_read<T>(
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

    /// «Да, удалены»: mark the vanished files missing (FR-FS-003).
    pub fn confirm_missing(&self) {
        lock(&self.shared).gone_question = None;
        let _ = self.jobs.send(Job::Reconcile { confirmed: true });
    }

    /// «Нет»: leave them as they are for now (the question returns on the next pass while the
    /// files are still away).
    pub fn dismiss_question(&self) {
        lock(&self.shared).gone_question = None;
    }

    /// Serves `sirtone-cover://<hash>/<size>` (docs/03 §5.6): WebP bytes, generated on first use.
    pub fn cover_bytes(&self, hash: &str, size: u32) -> Option<Vec<u8>> {
        let root = lock(&self.shared).root.clone()?;
        let path = sirtone_library::covers::variant(&root, hash, size).ok()?;
        std::fs::read(path).ok()
    }

    /// Library tracks heard since `since_ms` (shuffle pushes them later, `01b` F-21).
    pub fn recent_track_ids(&self, since_ms: i64) -> std::collections::HashSet<i64> {
        self.with_read(|c| {
            let mut st =
                c.prepare_cached("SELECT DISTINCT track_id FROM play_event WHERE ts >= ?1")?;
            let ids = st
                .query_map([since_ms], |r| r.get::<_, i64>(0))?
                .collect::<Result<_, _>>()?;
            Ok(ids)
        })
        .unwrap_or_default()
    }

    /// 512 px JPEG of an album cover for the Windows media controls (docs/07 §3).
    pub fn smtc_cover(&self, hash: &str) -> Option<PathBuf> {
        let root = lock(&self.shared).root.clone()?;
        let dir = crate::diagnostics::data_dir().join("cache").join("smtc");
        sirtone_library::covers::smtc_jpeg(&root, hash, &dir).ok()
    }

    /// Absolute paths + display data + album cover hash for track ids (in that order).
    /// The online add-on (D-87): a track confirmed by MusicBrainz moves to its place on the
    /// import worker, then its identity is stamped «confirmed».
    pub fn assign_online(
        &self,
        track: i64,
        assignment: sirtone_library::relocate::Assignment,
        mbid: String,
        confidence: f64,
        release_group: Option<String>,
    ) -> Result<String, AppError> {
        self.assign_with(track, assignment, Some((mbid, confidence, release_group)))
    }

    /// An assignment on the import worker (journalled move), optionally confirmed online.
    pub(crate) fn assign_with(
        &self,
        track: i64,
        assignment: sirtone_library::relocate::Assignment,
        online: Option<(String, f64, Option<String>)>,
    ) -> Result<String, AppError> {
        let (tx, rx) = mpsc::channel();
        self.jobs
            .send(Job::Assign {
                track,
                assignment,
                online,
                reply: tx,
            })
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        rx.recv_timeout(Duration::from_secs(120))
            .map_err(|_| AppError::new(ErrorCode::Internal).with_details("assignment timed out"))?
    }

    /// Playable tracks by id as list rows, in the given order.
    pub fn track_items(&self, ids: &[u32]) -> Result<Vec<TrackItem>, AppError> {
        let ids: Vec<i64> = ids.iter().map(|&i| i64::from(i)).collect();
        self.with_read(|c| {
            Ok(collection::tracks_by_ids(c, &ids)?
                .into_iter()
                .map(track_item)
                .collect())
        })
    }

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
                        (SELECT cv.hash FROM album al JOIN cover cv ON cv.id = al.cover_id WHERE al.id = t.album_id),
                        t.album_id,
                        (SELECT ta.artist_id FROM track_artist ta WHERE ta.track_id = t.id ORDER BY ta.position LIMIT 1)
                 FROM track t WHERE t.id = ?1",
            )?;
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Ok(r) = st.query_row([id], |r| {
                    Ok(Resolved {
                        id: *id,
                        path: r
                            .get::<_, String>(0)?
                            .split('/')
                            .fold(root.clone(), |p, c| p.join(c)),
                        title: r.get(1)?,
                        artists: r.get(2)?,
                        cover: r.get(3)?,
                        album_id: r.get(4)?,
                        artist_id: r.get(5)?,
                        art: 0,
                    })
                }) {
                    out.push(r);
                }
            }
            let ids: Vec<i64> = out.iter().map(|r| i64::from(r.id)).collect();
            let art = sirtone_library::art::track_hints(c, &ids)?;
            for r in &mut out {
                r.art = art.get(&i64::from(r.id)).copied().unwrap_or(0x70);
            }
            Ok(out)
        })
    }
}

/// Watches whether the configured root is reachable (a pulled stick kills the watch silently):
/// online → offline drops connections and the watcher; offline → online reopens them and
/// reconciles (FR-FS-004).
fn root_monitor(
    app: AppHandle,
    shared: Arc<Mutex<Shared>>,
    settings: Arc<SettingsStore>,
    jobs: Sender<Job>,
) {
    loop {
        std::thread::sleep(Duration::from_secs(3));
        let (root, offline) = {
            let s = lock(&shared);
            (s.root.clone(), s.offline)
        };
        let Some(root) = root else { continue };
        let reachable = root.is_dir() && db_path(&root).is_file();
        if offline && reachable {
            let uid = settings_uid(&settings);
            if let Some(conn) = open_known(&root, uid.as_deref()) {
                remember_uid(&settings, &conn);
                let watch = arm_watch(&root, &jobs);
                {
                    let mut s = lock(&shared);
                    s.read = Some(conn);
                    s.offline = false;
                    s.counts = None;
                    s.watch = watch;
                }
                tracing::info!(target: "sirtone::library", "library folder back online");
                let _ = jobs.send(Job::Reconcile { confirmed: false });
                emit_changed(&app, &shared);
            }
        } else if !offline && !reachable {
            go_offline(&app, &shared);
        }
    }
}

fn go_offline(app: &AppHandle, shared: &Mutex<Shared>) {
    {
        let mut s = lock(shared);
        if s.offline {
            return;
        }
        s.offline = true;
        s.read = None;
        s.watch = None;
        s.counts = None;
        s.gone_question = None;
    }
    tracing::warn!(target: "sirtone::library", "library folder unavailable — offline");
    let _ = LibraryChanged { tracks: 0 }.emit(app);
}

fn import_worker(
    app: AppHandle,
    shared: Arc<Mutex<Shared>>,
    rx: mpsc::Receiver<Job>,
    cancel: Arc<AtomicBool>,
    importing: Arc<AtomicBool>,
) {
    let deps = deps();
    let mut conn: Option<(PathBuf, Connection)> = None;
    let mut backfilled: Option<PathBuf> = None;
    while let Ok(job) = rx.recv() {
        // Coalesce queued reconcile passes (a burst of watcher events = one pass).
        let mut jobs = vec![job];
        while let Ok(more) = rx.try_recv() {
            jobs.push(more);
        }
        let mut reconcile_pass: Option<bool> = None;
        let mut imports: Vec<PathBuf> = Vec::new();
        let mut assigns = Vec::new();
        let mut merges = Vec::new();
        let mut organize = Vec::new();
        for j in jobs {
            match j {
                Job::Import(p) => imports.extend(p),
                Job::Assign {
                    track,
                    assignment,
                    online,
                    reply,
                } => assigns.push((track, Some(assignment), online, false, reply)),
                Job::Remove {
                    track,
                    with_file,
                    reply,
                } => assigns.push((track, None, None, with_file, reply)),
                Job::MergeArtists { keep, drop, reply } => {
                    merges.push((Some((keep, drop)), None, Some(reply), None))
                }
                Job::UndoMerge { record, reply } => {
                    merges.push((None, Some(record), None, Some(reply)))
                }
                Job::Organize { all, reply } => organize.push((all, reply)),
                Job::Reconcile { confirmed } => {
                    reconcile_pass = Some(reconcile_pass.unwrap_or(false) || confirmed);
                }
            }
        }
        let (root, offline) = {
            let s = lock(&shared);
            (s.root.clone(), s.offline)
        };
        let Some(root) = root else {
            continue;
        };
        if offline {
            conn = None;
            continue;
        }
        if conn.as_ref().is_none_or(|(r, _)| *r != root) {
            conn = sirtone_library::open_existing(&db_path(&root), medium(&root))
                .ok()
                .map(|c| (root.clone(), c));
        }
        let Some((_, db)) = conn.as_mut() else {
            continue;
        };
        // Artist merges: the database first, then each track's file on its own (a locked or
        // playing file stays where it is and moves on the next merge or «навести порядок»).
        if !merges.is_empty() {
            for (pair, record, merge_reply, undo_reply) in merges {
                let relayout = |db: &mut rusqlite::Connection, tracks: &[i64]| -> u32 {
                    let mut failed = 0;
                    for &t in tracks {
                        if sirtone_library::relocate::relayout(db, &root, t, deps.fs.as_ref())
                            .is_err()
                        {
                            failed += 1;
                        }
                    }
                    failed
                };
                if let (Some((keep, drop)), Some(reply)) = (pair, merge_reply) {
                    let r = sirtone_library::artists::merge(db, keep, drop)
                        .map_err(AppError::from)
                        .map(|rec| {
                            let failed = relayout(db, &rec.tracks);
                            (rec, failed)
                        });
                    let _ = reply.send(r);
                } else if let (Some(rec), Some(reply)) = (record, undo_reply) {
                    let r = sirtone_library::artists::undo(db, &rec)
                        .map_err(AppError::from)
                        .map(|tracks| relayout(db, &tracks));
                    let _ = reply.send(r);
                }
            }
            emit_changed(&app, &shared);
            refresh_queue(&app);
        }
        for (all, reply) in organize {
            let tracks: Vec<i64> = if all {
                db.prepare("SELECT id FROM track WHERE state = 'ok' ORDER BY id")
                    .and_then(|mut st| {
                        st.query_map([], |r| r.get(0))?
                            .collect::<Result<Vec<i64>, _>>()
                    })
                    .unwrap_or_default()
            } else {
                std::mem::take(&mut lock(&shared).organize)
            };
            let moved = tracks
                .iter()
                .filter(|&&t| {
                    sirtone_library::relocate::relayout(db, &root, t, deps.fs.as_ref())
                        .unwrap_or(false)
                })
                .count() as u32;
            let _ = reply.send(Ok(moved));
            if moved > 0 {
                emit_changed(&app, &shared);
                refresh_queue(&app);
            }
        }
        if !assigns.is_empty() {
            for (track, assignment, online, with_file, reply) in assigns {
                let r = match assignment {
                    Some(a) => {
                        sirtone_library::relocate::assign(db, &root, track, &a, deps.fs.as_ref())
                            .and_then(|path| {
                                if let Some((mbid, conf, rg)) = &online {
                                    sirtone_library::online::confirm(
                                        db,
                                        track,
                                        mbid,
                                        *conf,
                                        rg.as_deref(),
                                    )?;
                                }
                                Ok(path)
                            })
                    }
                    None => sirtone_library::relocate::remove(
                        db,
                        &root,
                        track,
                        with_file,
                        deps.fs.as_ref(),
                    )
                    .map(|()| String::new()),
                }
                .map_err(AppError::from);
                if let Err(e) = &r {
                    tracing::warn!(target: "sirtone::library", code = ?e.code, "assignment failed");
                }
                let _ = reply.send(r);
            }
            emit_changed(&app, &shared);
            refresh_queue(&app);
        }
        let paths = imports;
        if let Err(e) = import::recover(db, &root, deps.fs.as_ref()) {
            tracing::error!(target: "sirtone::import", error = %e, "recovery failed");
        }
        // Once per library: covers for albums imported before T1.26 (or without art on their
        // first track).
        if backfilled.as_deref() != Some(root.as_path()) {
            backfilled = Some(root.clone());
            if let Ok(n) = sirtone_library::covers::backfill(db, &root, deps.covers.as_ref(), 500)
                && n > 0
            {
                tracing::info!(target: "sirtone::import", albums = n, "covers backfilled");
                emit_changed(&app, &shared);
            }
        }
        let mut batches = import::unfinished_batches(db).unwrap_or_default();
        importing.store(!batches.is_empty() || !paths.is_empty(), Ordering::Relaxed);
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
                        batch: r.batch_id as u32,
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
                    // New «Неизвестные» are looked up online when there is a network (D-87).
                    if r.unknown > 0 {
                        crate::resolve::kick(&app);
                    }
                }
                Err(e) => tracing::error!(target: "sirtone::import", error = %e, "batch failed"),
            }
            let _ = added_since;
            emit_changed(&app, &shared);
        }
        cancel.store(false, Ordering::Relaxed);
        importing.store(false, Ordering::Relaxed);
        if let Some(confirmed) = reconcile_pass {
            run_reconcile(&app, &shared, db, &root, &deps, confirmed);
        }
    }
}

/// One reconcile pass after the journal is settled (`recover` runs first on every job).
fn run_reconcile(
    app: &AppHandle,
    shared: &Mutex<Shared>,
    db: &mut Connection,
    root: &Path,
    deps: &ImportDeps,
    confirmed: bool,
) {
    if let Err(e) = import::recover(db, root, deps.fs.as_ref()) {
        tracing::error!(target: "sirtone::import", error = %e, "recovery failed");
        return;
    }
    let started = Instant::now();
    match sirtone_library::reconcile::reconcile(
        db,
        root,
        deps,
        sirtone_library::db::now_ms(),
        confirmed,
    ) {
        Ok(Outcome::Offline) => go_offline(app, shared),
        Ok(Outcome::Done(r)) => {
            tracing::info!(target: "sirtone::library", ?r, ms = started.elapsed().as_millis() as u64, "reconcile");
            organize_adopted(app, shared, db, root, deps, &r.adopted_ids);
            if r != Report::default() {
                emit_changed(app, shared);
                refresh_queue(app);
            }
        }
        Ok(Outcome::Question { gone, report }) => {
            tracing::warn!(target: "sirtone::library", gone, "many files vanished — asking");
            lock(shared).gone_question = Some(gone as u32);
            let _ = report;
            emit_changed(app, shared);
        }
        Err(e) => tracing::error!(target: "sirtone::library", error = %e, "reconcile failed"),
    }
}

/// FR-FS-005 (T3.19): files added in Explorer are taken where they are; those the rules would
/// place elsewhere are moved at once (`organize_external` = `auto`), offered (`suggest`, the
/// default — never moved silently) or left (`never`).
fn organize_adopted(
    app: &AppHandle,
    shared: &Mutex<Shared>,
    db: &mut Connection,
    root: &Path,
    deps: &ImportDeps,
    adopted: &[i64],
) {
    if adopted.is_empty() {
        return;
    }
    let mode = tauri::Manager::try_state::<Arc<SettingsStore>>(app)
        .and_then(|s| {
            s.get()["library"]["organize_external"]
                .as_str()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "suggest".into());
    if mode == "never" {
        return;
    }
    let off: Vec<i64> = adopted
        .iter()
        .copied()
        .filter(|&t| sirtone_library::relocate::out_of_place(db, t).unwrap_or(false))
        .collect();
    if off.is_empty() {
        return;
    }
    if mode == "auto" {
        for t in off {
            let _ = sirtone_library::relocate::relayout(db, root, t, deps.fs.as_ref());
        }
        return;
    }
    let total = {
        let mut s = lock(shared);
        for t in off {
            if !s.organize.contains(&t) {
                s.organize.push(t);
            }
        }
        s.organize.len() as u32
    };
    let _ = OrganizeSuggested { tracks: total }.emit(app);
}

/// Files moved: queued tracks take their new paths (the playing one keeps its own).
fn refresh_queue(app: &AppHandle) {
    if let (Some(pb), Some(lib)) = (
        tauri::Manager::try_state::<crate::playback::Playback>(app),
        tauri::Manager::try_state::<Library>(app),
    ) {
        pb.refresh_from(&lib);
    }
}

/// Lists and counters must refresh. The full counters (163 ms on 100 000 tracks) are only
/// invalidated here and recomputed lazily by the next `library_status` — never under the lock on
/// every import tick (deferred from T1.20).
impl Library {
    /// The lists refetch (a change made outside the import worker: quarantine, «нормальный»).
    pub(crate) fn notify_changed(&self, app: &AppHandle) {
        emit_changed(app, &self.shared);
    }
}

fn emit_changed(app: &AppHandle, shared: &Mutex<Shared>) {
    let mut s = lock(shared);
    s.counts = None;
    let tracks = s
        .read
        .as_ref()
        .and_then(|c| repo::track_count(c).ok())
        .unwrap_or(0) as u32;
    drop(s);
    let _ = LibraryChanged { tracks }.emit(app);
}

// ---------- commands ----------

pub(crate) fn track_item(r: repo::TrackRow) -> TrackItem {
    TrackItem {
        id: r.id as u32,
        title: r.title,
        artists: r.artists,
        album: r.album,
        duration_ms: u32::try_from(r.duration_ms).unwrap_or(u32::MAX),
        album_id: r.album_id.map(|v| v as u32),
        artist_id: r.artist_id.map(|v| v as u32),
    }
}

#[tauri::command(async)]
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

#[tauri::command(async)]
#[specta::specta]
pub fn library_set_root(lib: State<'_, Library>, path: String) -> Result<LibraryStatus, AppError> {
    lib.set_root(Path::new(&path))?;
    Ok(lib.status())
}

#[tauri::command(async)]
#[specta::specta]
pub fn import_paths(app: AppHandle, lib: State<'_, Library>, paths: Vec<String>) -> ImportStart {
    lib.import_paths(&app, paths.into_iter().map(PathBuf::from).collect())
}

#[tauri::command]
#[specta::specta]
pub fn library_confirm_missing(lib: State<'_, Library>) {
    lib.confirm_missing();
}

#[tauri::command]
#[specta::specta]
pub fn library_dismiss_question(lib: State<'_, Library>) {
    lib.dismiss_question();
}

#[tauri::command]
#[specta::specta]
pub fn import_cancel(lib: State<'_, Library>) {
    lib.cancel.store(true, Ordering::Relaxed);
}

#[tauri::command(async)]
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

#[tauri::command(async)]
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
        let ids: Vec<i64> = rows
            .iter()
            .filter(|a| a.cover.is_none())
            .map(|a| a.id)
            .collect();
        let art = sirtone_library::art::album_hints(c, &ids)?;
        Ok(AlbumPage {
            rows: rows
                .into_iter()
                .map(|a| AlbumItem {
                    id: a.id as u32,
                    title: a.title,
                    artist: a.artist,
                    year: a.year,
                    tracks: a.tracks,
                    art: art.get(&a.id).copied().unwrap_or(0x70),
                    cover: a.cover,
                    added_at: a.added_at as f64,
                })
                .collect(),
            next: next.map(|n| PageCursor {
                key: n.key,
                id: n.id as u32,
            }),
        })
    })
}

#[tauri::command(async)]
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
                    added_at: a.added_at as f64,
                })
                .collect(),
            next: next.map(|n| PageCursor {
                key: n.key,
                id: n.id as u32,
            }),
        })
    })
}

/// A track in «Неизвестные» with its reason (docs/05 §4.7).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UnknownItem {
    pub track: TrackItem,
    /// no_metadata | conflict | low_confidence | offline_pending | possible_non_track |
    /// needs_review | quarantined (stopped playing, FR-IMP-008)
    pub reason: Option<String>,
    /// Hints in one click (T3.11): who it may be, best first.
    pub candidates: Vec<UnknownCandidate>,
}

/// A hint for an unknown track (from the online lookup, D-87).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UnknownCandidate {
    pub title: String,
    /// «A, B».
    pub artists: String,
    /// From MusicBrainz (accepting it confirms the identity).
    pub online: bool,
}

#[tauri::command(async)]
#[specta::specta]
pub fn unknown_list(lib: State<'_, Library>) -> Result<Vec<UnknownItem>, AppError> {
    lib.with_read(|c| {
        let mut out = Vec::new();
        for u in collection::unknown_rows(c, 5000)? {
            let candidates = sirtone_library::unknowns::candidates(c, u.track.id)?
                .into_iter()
                .take(3)
                .map(|k| UnknownCandidate {
                    title: k.title,
                    artists: k.artists.join(", "),
                    online: k.mbid.is_some(),
                })
                .collect();
            out.push(UnknownItem {
                track: track_item(u.track),
                reason: u.reason,
                candidates,
            });
        }
        // FR-IMP-008: files that stopped playing wait for the user's word.
        for t in sirtone_library::unknowns::quarantined(c, 1000)? {
            out.push(UnknownItem {
                track: track_item(t),
                reason: Some("quarantined".into()),
                candidates: Vec::new(),
            });
        }
        Ok(out)
    })
}

/// What the user typed for a track (ручное назначение).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackAssignment {
    pub title: String,
    /// Several artists separated by commas.
    pub artists: String,
    pub album: Option<String>,
    pub track_no: Option<u32>,
    pub year: Option<i32>,
}

/// «Назначить»: the track leaves «Неизвестные» and its file moves to the canonical place
/// (through the journal, on the import worker; the audio file itself is not changed).
#[tauri::command(async)]
#[specta::specta]
pub fn track_assign(
    lib: State<'_, Library>,
    track: u32,
    assignment: TrackAssignment,
) -> Result<(), AppError> {
    let (tx, rx) = mpsc::channel();
    lib.jobs
        .send(Job::Assign {
            track: i64::from(track),
            assignment: sirtone_library::relocate::Assignment {
                title: assignment.title,
                artists: assignment.artists,
                album: assignment.album,
                track_no: assignment.track_no,
                year: assignment.year,
            },
            online: None,
            reply: tx,
        })
        .map_err(|_| AppError::new(ErrorCode::Internal))?;
    rx.recv_timeout(Duration::from_secs(60))
        .map_err(|_| AppError::new(ErrorCode::Internal).with_details("assignment timed out"))?
        .map(|_| ())
}

/// Everything the track card shows (FR-LIB-006).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TrackDetails {
    pub id: u32,
    pub title: String,
    pub artists: String,
    pub album: Option<String>,
    pub track_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub codec: String,
    pub container: String,
    pub lossless: bool,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub duration_ms: u32,
    /// Bytes as f64 (u64 is not a JS number).
    pub size_bytes: f64,
    /// Absolute path of the file.
    pub path: String,
    pub added_at: f64,
    pub identity: String,
    /// «К альбому» from the player.
    pub album_id: Option<u32>,
}

#[tauri::command(async)]
#[specta::specta]
pub fn track_details(lib: State<'_, Library>, track: u32) -> Result<TrackDetails, AppError> {
    let root = lock(&lib.shared)
        .root
        .clone()
        .ok_or_else(|| AppError::new(ErrorCode::LibraryRootNotSet))?;
    lib.with_read(|c| {
        Ok(c.query_row(
            "SELECT t.id, t.title,
                    coalesce((SELECT group_concat(name, ', ') FROM
                      (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                       WHERE ta.track_id = t.id ORDER BY ta.position)), ''),
                    al.title, t.track_no, coalesce(t.year, al.year), t.genre_main, t.codec, t.container,
                    t.lossless, t.sample_rate, t.channels, t.duration_ms, t.size_bytes, t.rel_path,
                    t.added_at, t.identity_state, t.album_id
             FROM track t LEFT JOIN album al ON al.id = t.album_id WHERE t.id = ?1",
            [track],
            |r| {
                let rel: String = r.get(14)?;
                Ok(TrackDetails {
                    id: r.get::<_, i64>(0)? as u32,
                    title: r.get(1)?,
                    artists: r.get(2)?,
                    album: r.get(3)?,
                    track_no: r.get(4)?,
                    year: r.get(5)?,
                    genre: r.get(6)?,
                    codec: r.get(7)?,
                    container: r.get(8)?,
                    lossless: r.get(9)?,
                    sample_rate: r.get(10)?,
                    channels: r.get(11)?,
                    duration_ms: u32::try_from(r.get::<_, i64>(12)?).unwrap_or(u32::MAX),
                    size_bytes: r.get::<_, i64>(13)? as f64,
                    path: rel
                        .split('/')
                        .fold(root.clone(), |p, c| p.join(c))
                        .to_string_lossy()
                        .into_owned(),
                    added_at: r.get::<_, i64>(15)? as f64,
                    identity: r.get(16)?,
                    album_id: r.get::<_, Option<i64>>(17)?.map(|a| a as u32),
                })
            },
        )?)
    })
}

impl Library {
    /// Artist merge on the import worker (T3.12): the record (for undo) and how many files did
    /// not move.
    pub(crate) fn merge_artists(
        &self,
        keep: i64,
        drop: i64,
    ) -> Result<(sirtone_library::artists::MergeRecord, u32), AppError> {
        let (tx, rx) = mpsc::channel();
        self.jobs
            .send(Job::MergeArtists {
                keep,
                drop,
                reply: tx,
            })
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        rx.recv_timeout(Duration::from_secs(300))
            .map_err(|_| AppError::new(ErrorCode::Internal).with_details("merge timed out"))?
    }

    pub(crate) fn undo_merge(
        &self,
        record: sirtone_library::artists::MergeRecord,
    ) -> Result<u32, AppError> {
        let (tx, rx) = mpsc::channel();
        self.jobs
            .send(Job::UndoMerge {
                record: Box::new(record),
                reply: tx,
            })
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        rx.recv_timeout(Duration::from_secs(300))
            .map_err(|_| AppError::new(ErrorCode::Internal).with_details("undo timed out"))?
    }

    /// A track leaves the library through the import worker (journalled): with its file to the
    /// Recycle Bin or only from the app (FR-DATA-001).
    pub(crate) fn remove_track(&self, track: i64, with_file: bool) -> Result<(), AppError> {
        let (tx, rx) = mpsc::channel();
        self.jobs
            .send(Job::Remove {
                track,
                with_file,
                reply: tx,
            })
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        rx.recv_timeout(Duration::from_secs(60))
            .map_err(|_| AppError::new(ErrorCode::Internal).with_details("removal timed out"))?
            .map(|_| ())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RejectedItem {
    pub path: String,
    /// The reason code (`import.reason.*`).
    pub reason: String,
}

/// The import report in full (T3.21, FR-IMP-006): added · «Неизвестные» · exact copies · not
/// added. Sound duplicates come later, after the analysis («Дубли», D-81).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub added: Vec<TrackItem>,
    pub unknown: Vec<TrackItem>,
    pub duplicates: Vec<String>,
    pub rejected: Vec<RejectedItem>,
}

#[tauri::command(async)]
#[specta::specta]
pub fn import_report(lib: State<'_, Library>, batch: u32) -> Result<ImportReport, AppError> {
    lib.with_read(|c| {
        let d = sirtone_library::import::batch_details(c, i64::from(batch))?;
        Ok(ImportReport {
            added: d.added.into_iter().map(track_item).collect(),
            unknown: d.unknown.into_iter().map(track_item).collect(),
            duplicates: d.duplicates,
            rejected: d
                .rejected
                .into_iter()
                .map(|(path, reason)| RejectedItem { path, reason })
                .collect(),
        })
    })
}

/// «Навести порядок» (T3.19, FR-FS-005): the adopted files waiting for it (`all` = false) or
/// every track; files go where the layout rules say, through the journal. Returns how many
/// moved.
#[tauri::command(async)]
#[specta::specta]
pub fn library_organize(lib: State<'_, Library>, all: bool) -> Result<u32, AppError> {
    let (tx, rx) = mpsc::channel();
    lib.jobs
        .send(Job::Organize { all, reply: tx })
        .map_err(|_| AppError::new(ErrorCode::Internal))?;
    rx.recv_timeout(Duration::from_secs(600))
        .map_err(|_| AppError::new(ErrorCode::Internal).with_details("organize timed out"))?
}

/// FR-DATA-001: «удалить файл с диска» (to the Recycle Bin) or «только убрать из приложения».
#[tauri::command(async)]
#[specta::specta]
pub fn track_remove(
    lib: State<'_, Library>,
    pb: State<'_, crate::playback::Playback>,
    track: u32,
    with_file: bool,
) -> Result<(), AppError> {
    pb.forget_tracks(&[track]);
    lib.remove_track(i64::from(track), with_file)
}

/// FR-DATA-002: what deleting an artist takes away — for the confirmation.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RemovalInfo {
    pub tracks: u32,
    /// Bytes as f64 (u64 is not a JS number).
    pub bytes: f64,
}

#[tauri::command(async)]
#[specta::specta]
pub fn artist_removal_info(lib: State<'_, Library>, artist: u32) -> Result<RemovalInfo, AppError> {
    let (ids, bytes) =
        lib.with_read(|c| sirtone_library::relocate::artist_tracks(c, i64::from(artist)))?;
    Ok(RemovalInfo {
        tracks: u32::try_from(ids.len()).unwrap_or(u32::MAX),
        bytes: bytes as f64,
    })
}

/// FR-DATA-002: every track on the artist's page goes (files to the Recycle Bin, or only from
/// the app); one worker job per track, so each is journalled on its own.
#[tauri::command(async)]
#[specta::specta]
pub fn artist_remove(
    lib: State<'_, Library>,
    pb: State<'_, crate::playback::Playback>,
    artist: u32,
    with_file: bool,
) -> Result<(), AppError> {
    let (ids, _) =
        lib.with_read(|c| sirtone_library::relocate::artist_tracks(c, i64::from(artist)))?;
    let forget: Vec<u32> = ids.iter().filter_map(|&i| u32::try_from(i).ok()).collect();
    pb.forget_tracks(&forget);
    let mut replies = Vec::with_capacity(ids.len());
    for track in ids {
        let (tx, rx) = mpsc::channel();
        lib.jobs
            .send(Job::Remove {
                track,
                with_file,
                reply: tx,
            })
            .map_err(|_| AppError::new(ErrorCode::Internal))?;
        replies.push(rx);
    }
    let mut first_err = None;
    for rx in replies {
        let r = rx
            .recv_timeout(Duration::from_secs(120))
            .map_err(|_| AppError::new(ErrorCode::Internal).with_details("removal timed out"))
            .and_then(|r| r);
        if let Err(e) = r {
            first_err.get_or_insert(e);
        }
    }
    first_err.map_or(Ok(()), Err)
}

/// «Показать в проводнике» (Q-23): Explorer opens the folder with the file selected.
#[tauri::command(async)]
#[specta::specta]
pub fn track_reveal(lib: State<'_, Library>, track: u32) -> Result<(), AppError> {
    let root = lock(&lib.shared)
        .root
        .clone()
        .ok_or_else(|| AppError::new(ErrorCode::LibraryRootNotSet))?;
    let rel: String = lib.with_read(|c| {
        Ok(
            c.query_row("SELECT rel_path FROM track WHERE id = ?1", [track], |r| {
                r.get(0)
            })?,
        )
    })?;
    let path = rel.split('/').fold(root, |p, c| p.join(c));
    reveal(&path)
}

#[cfg(windows)]
fn reveal(path: &Path) -> Result<(), AppError> {
    use std::os::windows::process::CommandExt;
    // Explorer parses `/select,"path"` itself; std quoting of the whole argument breaks it.
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e.to_string()))
}

#[cfg(not(windows))]
fn reveal(_path: &Path) -> Result<(), AppError> {
    Err(AppError::new(ErrorCode::Internal).with_details("explorer is Windows-only"))
}

#[tauri::command(async)]
#[specta::specta]
pub fn unknown_tracks(lib: State<'_, Library>) -> Result<Vec<TrackItem>, AppError> {
    lib.with_read(|c| {
        Ok(collection::unknown_tracks(c, 5000)?
            .into_iter()
            .map(track_item)
            .collect())
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn album_tracks(lib: State<'_, Library>, album: u32) -> Result<Vec<TrackItem>, AppError> {
    lib.with_read(|c| {
        Ok(collection::album_tracks(c, i64::from(album))?
            .into_iter()
            .map(track_item)
            .collect())
    })
}

/// A genre card (FR-LIB-002; `01b` F-34).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GenreItem {
    /// The page address (normalised name).
    pub key: String,
    pub name: String,
    pub tracks: u32,
}

#[tauri::command(async)]
#[specta::specta]
pub fn collection_genres(lib: State<'_, Library>) -> Result<Vec<GenreItem>, AppError> {
    lib.with_read(|c| {
        Ok(collection::genres(c)?
            .into_iter()
            .map(|g| GenreItem {
                key: g.key,
                name: g.name,
                tracks: g.tracks,
            })
            .collect())
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn genre_tracks(lib: State<'_, Library>, key: String) -> Result<Vec<TrackItem>, AppError> {
    lib.with_read(|c| {
        Ok(collection::genre_tracks(c, &key)?
            .into_iter()
            .map(track_item)
            .collect())
    })
}

#[tauri::command(async)]
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
            // «Авто» reads the pixels on a canvas (T2.2): a cross-origin image must allow it.
            .header("Access-Control-Allow-Origin", "*")
            .body(bytes),
        None => tauri::http::Response::builder()
            .status(404)
            .body(Vec::new()),
    }
    .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}
