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

/// Work for the import thread: imports and reconcile passes never run at the same time, so the
/// pipeline's own placements can never race the watcher (docs/05 §7).
enum Job {
    Import(Vec<PathBuf>),
    Reconcile { confirmed: bool },
}

/// A library track ready to play: absolute path, title, artists, album cover hash.
pub type Resolved = (PathBuf, String, String, Option<String>);

struct Shared {
    root: Option<PathBuf>,
    read: Option<Connection>,
    counts: Option<Counts>,
    pending: Vec<PathBuf>,
    offline: bool,
    gone_question: Option<u32>,
    watch: Option<sirtone_library::watch::Watch>,
}

pub struct Library {
    shared: Arc<Mutex<Shared>>,
    settings: Arc<SettingsStore>,
    jobs: Sender<Job>,
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

fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
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

    /// 512 px JPEG of an album cover for the Windows media controls (docs/07 §3).
    pub fn smtc_cover(&self, hash: &str) -> Option<PathBuf> {
        let root = lock(&self.shared).root.clone()?;
        let dir = crate::diagnostics::data_dir().join("cache").join("smtc");
        sirtone_library::covers::smtc_jpeg(&root, hash, &dir).ok()
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
        for j in jobs {
            match j {
                Job::Import(p) => imports.extend(p),
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
            if r != Report::default() {
                emit_changed(app, shared);
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

/// Lists and counters must refresh. The full counters (163 ms on 100 000 tracks) are only
/// invalidated here and recomputed lazily by the next `library_status` — never under the lock on
/// every import tick (deferred from T1.20).
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

fn track_item(r: repo::TrackRow) -> TrackItem {
    TrackItem {
        id: r.id as u32,
        title: r.title,
        artists: r.artists,
        album: r.album,
        duration_ms: u32::try_from(r.duration_ms).unwrap_or(u32::MAX),
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
            .body(bytes),
        None => tauri::http::Response::builder()
            .status(404)
            .body(Vec::new()),
    }
    .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}
