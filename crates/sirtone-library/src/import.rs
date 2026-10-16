//! Import pipeline (T1.11–T1.14, docs/05 §3, docs/03 §8.3/§15.2, D-66).
//!
//! Discovered → Probed → Copied (staging, BLAKE3 verified on disk) → Identified → Placed (journal)
//! → Indexed. Sources are opened read-only and never changed; the only files this module deletes
//! are its own inside `<root>/.sirtone/staging/`. Several probe+copy workers, one writer thread
//! (this one) for the database and placement.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use rusqlite::{Connection, OptionalExtension, params};
use sirtone_core::error::ErrorCode;
use sirtone_core::ports::{
    CoverReader, FileOps, FolderHints, Identify, MediaInfo, MediaProbe, TagReader, VolumeInfo,
};

use crate::db::now_ms;
use crate::error::LibraryError;
use crate::layout::{self, MAX_REL_PATH, TrackPlace};
use crate::repo::{self, Identity, NewTrack};

/// Hidden service folder inside the library root (docs/03 §8.1).
pub const SERVICE_DIR: &str = ".sirtone";
const COPY_BUF: usize = 1 << 20;
const WORKERS: usize = 3;
/// Extensions that are never audio for import purposes (FR-IMP-004).
const PLAYLISTS: &[&str] = &["m3u", "m3u8", "pls", "cue", "xspf", "wpl"];

/// «Не добавлено» reasons (docs/05 §3.2), stable codes for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    NotAudio,
    Corrupt,
    Unsupported,
    Playlist,
    NoSpace,
    PathTooLong,
    ReadError,
}

impl Rejected {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotAudio => "not_audio",
            Self::Corrupt => "corrupt",
            Self::Unsupported => "unsupported",
            Self::Playlist => "playlist",
            Self::NoSpace => "no_space",
            Self::PathTooLong => "path_too_long",
            Self::ReadError => "read_error",
        }
    }
}

pub struct ImportDeps {
    pub probe: Arc<dyn MediaProbe>,
    pub tags: Arc<dyn TagReader>,
    pub identify: Arc<dyn Identify>,
    pub fs: Arc<dyn FileOps>,
    pub volume: Arc<dyn VolumeInfo>,
    pub covers: Arc<dyn CoverReader>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportEvent {
    /// Items finished (any final state) out of the batch total.
    Progress { done: u64, total: u64 },
    /// A track is in the library and playable right away (FR-IMP-005).
    TrackAdded { track_id: i64, unknown: bool },
    /// Every source file has been copied and verified (or rejected): the medium can be removed.
    SourcesReleased,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub batch_id: i64,
    pub added: u64,
    pub unknown: u64,
    pub duplicates: Vec<String>,
    pub rejected: Vec<(String, String)>,
    pub cancelled: u64,
    pub can_eject: bool,
}

fn staging_dir(root: &Path, batch: i64) -> PathBuf {
    root.join(SERVICE_DIR)
        .join("staging")
        .join(batch.to_string())
}

fn to_rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn from_rel(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c))
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Step 1 (Discovered): creates a batch and one `import_item` per file found under `sources`
/// (folders are walked recursively; hidden service folders skipped).
pub fn start_batch(
    conn: &mut Connection,
    sources: &[PathBuf],
    kind: &str,
) -> Result<i64, LibraryError> {
    let mut files = Vec::new();
    for s in sources {
        walk(s, &mut files);
    }
    files.sort();
    files.dedup();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO import_batch(started_at, source) VALUES (?1, ?2)",
        params![now_ms(), kind],
    )?;
    let batch = tx.last_insert_rowid();
    {
        let mut st = tx.prepare(
            "INSERT OR IGNORE INTO import_item(batch_id, src_path, size_bytes, state) VALUES (?1, ?2, ?3, 'discovered')",
        )?;
        for f in &files {
            let size = fs::metadata(f).map(|m| m.len() as i64).ok();
            st.execute(params![batch, f.to_string_lossy(), size])?;
        }
    }
    tx.commit()?;
    Ok(batch)
}

fn walk(p: &Path, out: &mut Vec<PathBuf>) {
    match fs::metadata(p) {
        Ok(m) if m.is_file() => out.push(p.to_path_buf()),
        Ok(m) if m.is_dir() => {
            let Ok(rd) = fs::read_dir(p) else { return };
            for e in rd.flatten() {
                let name = e.file_name();
                if name.to_string_lossy().starts_with(SERVICE_DIR) {
                    continue;
                }
                walk(&e.path(), out);
            }
        }
        _ => {}
    }
}

/// Batches that did not finish (crash, kill, power loss): resume them with [`run_batch`].
pub fn unfinished_batches(conn: &Connection) -> Result<Vec<i64>, LibraryError> {
    let mut st =
        conn.prepare("SELECT id FROM import_batch WHERE finished_at IS NULL ORDER BY id")?;
    let ids = st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    Ok(ids)
}

/// Worker output for one item.
enum Copied {
    Ok {
        item: i64,
        staged: PathBuf,
        hash: String,
        info: MediaInfo,
        src: PathBuf,
    },
    Rejected {
        item: i64,
        reason: Rejected,
    },
    Cancelled {
        item: i64,
    },
}

/// Copies `src` to `<dst>.part` hashing on the fly, flushes it to disk, renames it to `dst` and
/// re-hashes the written copy («хэши сошлись», docs/05 §3.2).
fn copy_verified(
    src: &Path,
    dst: &Path,
    fsops: &dyn FileOps,
    cancel: &AtomicBool,
) -> Result<Option<String>, Rejected> {
    let part = dst.with_extension(format!(
        "{}.part",
        dst.extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    let mut input = File::open(src).map_err(|_| Rejected::ReadError)?;
    let mut out = File::create(&part).map_err(|e| {
        if e.raw_os_error() == Some(112) {
            Rejected::NoSpace
        } else {
            Rejected::ReadError
        }
    })?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; COPY_BUF];
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(out);
            let _ = fs::remove_file(&part);
            return Ok(None);
        }
        let n = input.read(&mut buf).map_err(|_| Rejected::ReadError)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| {
            if e.raw_os_error() == Some(112) {
                Rejected::NoSpace
            } else {
                Rejected::ReadError
            }
        })?;
    }
    out.sync_all().map_err(|_| Rejected::ReadError)?;
    drop(out);
    let _ = fs::remove_file(dst); // a stale copy from an interrupted run (staging only)
    fsops
        .move_durable(&part, dst)
        .map_err(|_| Rejected::ReadError)?;
    let written = hash_file(dst).map_err(|_| Rejected::ReadError)?;
    let read = hasher.finalize().to_hex().to_string();
    if written != read {
        let _ = fs::remove_file(dst);
        return Err(Rejected::ReadError);
    }
    Ok(Some(read))
}

pub fn hash_file(p: &Path) -> std::io::Result<String> {
    let mut f = File::open(p)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; COPY_BUF];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

fn probe_and_copy(
    item: i64,
    src: PathBuf,
    staging: &Path,
    deps: &ImportDeps,
    cancel: &AtomicBool,
) -> Copied {
    if cancel.load(Ordering::Relaxed) {
        return Copied::Cancelled { item };
    }
    let ext = ext_of(&src);
    if PLAYLISTS.contains(&ext.as_str()) {
        return Copied::Rejected {
            item,
            reason: Rejected::Playlist,
        };
    }
    let info = match deps.probe.probe(&src) {
        Ok(i) => i,
        Err(e) => {
            let reason = match e.code {
                ErrorCode::UnsupportedFormat => Rejected::NotAudio,
                ErrorCode::DecodeFailed => Rejected::Corrupt,
                ErrorCode::FileNotFound | ErrorCode::PermissionDenied => Rejected::ReadError,
                _ => Rejected::Unsupported,
            };
            return Copied::Rejected { item, reason };
        }
    };
    if info.duration_ms == 0 {
        return Copied::Rejected {
            item,
            reason: Rejected::Corrupt,
        };
    }
    let staged = staging.join(if ext.is_empty() {
        item.to_string()
    } else {
        format!("{item}.{ext}")
    });
    match copy_verified(&src, &staged, deps.fs.as_ref(), cancel) {
        Ok(Some(hash)) => Copied::Ok {
            item,
            staged,
            hash,
            info,
            src,
        },
        Ok(None) => Copied::Cancelled { item },
        Err(reason) => Copied::Rejected { item, reason },
    }
}

struct ItemRow {
    id: i64,
    src: PathBuf,
    state: String,
    hash: Option<String>,
}

fn items(conn: &Connection, batch: i64) -> Result<Vec<ItemRow>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT id, src_path, state, content_hash FROM import_item WHERE batch_id = ?1 ORDER BY id",
    )?;
    let rows = st
        .query_map([batch], |r| {
            Ok(ItemRow {
                id: r.get(0)?,
                src: PathBuf::from(r.get::<_, String>(1)?),
                state: r.get(2)?,
                hash: r.get(3)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

fn set_state(
    conn: &Connection,
    item: i64,
    state: &str,
    reason: Option<&str>,
) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE import_item SET state = ?2, reason = coalesce(?3, reason) WHERE id = ?1",
        params![item, state, reason],
    )?;
    Ok(())
}

/// Runs (or resumes) a batch to the end or until `cancel`. Already placed/rejected items are
/// skipped; copied items whose staged file survived go straight to placement.
pub fn run_batch(
    conn: &mut Connection,
    root: &Path,
    batch: i64,
    deps: &ImportDeps,
    cancel: &AtomicBool,
    on_event: &mut dyn FnMut(ImportEvent),
) -> Result<Report, LibraryError> {
    let staging = staging_dir(root, batch);
    fs::create_dir_all(&staging)?;
    let all = items(conn, batch)?;
    let total = all.len() as u64;
    let mut done = all
        .iter()
        .filter(|i| !matches!(i.state.as_str(), "discovered" | "copied"))
        .count() as u64;

    // Resume: copied items with a surviving, matching staged file are placed directly.
    let mut ready: Vec<Copied> = Vec::new();
    let mut queue: VecDeque<(i64, PathBuf)> = VecDeque::new();
    for it in &all {
        match it.state.as_str() {
            "discovered" => queue.push_back((it.id, it.src.clone())),
            "copied" => {
                let staged = staging.join({
                    let ext = ext_of(&it.src);
                    if ext.is_empty() {
                        it.id.to_string()
                    } else {
                        format!("{}.{ext}", it.id)
                    }
                });
                let ok =
                    staged.exists() && it.hash.as_deref() == hash_file(&staged).ok().as_deref();
                match (ok, deps.probe.probe(&staged)) {
                    (true, Ok(info)) => ready.push(Copied::Ok {
                        item: it.id,
                        staged,
                        hash: it.hash.clone().unwrap_or_default(),
                        info,
                        src: it.src.clone(),
                    }),
                    _ => queue.push_back((it.id, it.src.clone())),
                }
            }
            _ => {}
        }
    }

    // Free space estimate (docs/05 §3.3): stop early with a clear reason instead of failing midway.
    let need: u64 = queue
        .iter()
        .filter_map(|(_, p)| fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    if let Some(free) = deps.volume.free_bytes(root)
        && need > free.saturating_sub(64 << 20)
    {
        for (item, _) in queue.drain(..) {
            set_state(conn, item, "rejected", Some(Rejected::NoSpace.as_str()))?;
            done += 1;
        }
    }

    // Folder consensus (docs/05 §4.1), computed once per source folder.
    let mut consensus: std::collections::HashMap<PathBuf, (Option<String>, Option<String>)> =
        std::collections::HashMap::new();
    let mut report = Report {
        batch_id: batch,
        ..Report::default()
    };
    let pending_copies = queue.len();
    let queue = Arc::new(Mutex::new(queue));
    let (tx, rx) = mpsc::channel::<Copied>();
    for c in ready {
        let _ = tx.send(c);
    }
    std::thread::scope(|scope| -> Result<(), LibraryError> {
        for _ in 0..WORKERS.min(pending_copies.max(1)) {
            let (queue, tx, staging) = (queue.clone(), tx.clone(), staging.clone());
            scope.spawn(move || {
                loop {
                    let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                    let Some((item, src)) = next else { break };
                    if tx
                        .send(probe_and_copy(item, src, &staging, deps, cancel))
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut copies_left = pending_copies;
        let mut released = pending_copies == 0;
        for msg in rx {
            match msg {
                Copied::Ok {
                    item,
                    staged,
                    hash,
                    info,
                    src,
                } => {
                    let from_queue = !matches!(all.iter().find(|i| i.id == item), Some(i) if i.state == "copied");
                    conn.execute(
                        "UPDATE import_item SET state = 'copied', content_hash = ?2 WHERE id = ?1",
                        params![item, hash],
                    )?;
                    if from_queue {
                        copies_left -= 1;
                    }
                    let hints = folder_hints(&src, deps.tags.as_ref(), &mut consensus);
                    place(
                        conn,
                        root,
                        batch,
                        item,
                        &staged,
                        &hash,
                        &info,
                        &src,
                        &hints,
                        deps,
                        &mut report,
                        on_event,
                    )?;
                }
                Copied::Rejected { item, reason } => {
                    copies_left -= 1;
                    set_state(conn, item, "rejected", Some(reason.as_str()))?;
                }
                Copied::Cancelled { item } => {
                    copies_left -= 1;
                    set_state(conn, item, "cancelled", None)?;
                }
            }
            done += 1;
            on_event(ImportEvent::Progress { done, total });
            if !released && copies_left == 0 && !cancel.load(Ordering::Relaxed) {
                released = true;
                on_event(ImportEvent::SourcesReleased);
            }
        }
        Ok(())
    })?;

    // Report from the item table (covers resumed runs too).
    {
        let mut st = conn.prepare(
            "SELECT i.src_path, i.state, i.reason, t.identity_state FROM import_item i
             LEFT JOIN track t ON t.id = i.track_id WHERE i.batch_id = ?1",
        )?;
        let mut q = st.query([batch])?;
        report.added = 0;
        report.unknown = 0;
        report.duplicates.clear();
        report.rejected.clear();
        report.cancelled = 0;
        while let Some(r) = q.next()? {
            let src: String = r.get(0)?;
            match r.get::<_, String>(1)?.as_str() {
                "placed" => {
                    report.added += 1;
                    if r.get::<_, Option<String>>(3)?.as_deref() == Some("unknown") {
                        report.unknown += 1;
                    }
                }
                "duplicate" => report.duplicates.push(src),
                "rejected" => report
                    .rejected
                    .push((src, r.get::<_, Option<String>>(2)?.unwrap_or_default())),
                "cancelled" => report.cancelled += 1,
                _ => {}
            }
        }
    }
    report.can_eject = report.cancelled == 0;
    let finished = cancel.load(Ordering::Relaxed)
        || conn.query_row(
            "SELECT count(*) FROM import_item WHERE batch_id = ?1 AND state IN ('discovered','copied')",
            [batch],
            |r| r.get::<_, i64>(0),
        )? == 0;
    if finished {
        conn.execute(
            "UPDATE import_batch SET finished_at = ?2, report_json = ?3 WHERE id = ?1",
            params![
                batch,
                now_ms(),
                format!(
                    "{{\"added\":{},\"unknown\":{},\"duplicates\":{},\"rejected\":{},\"cancelled\":{}}}",
                    report.added,
                    report.unknown,
                    report.duplicates.len(),
                    report.rejected.len(),
                    report.cancelled
                )
            ],
        )?;
        // Leftovers of a cancelled batch are our own staged copies.
        let _ = fs::remove_dir_all(&staging);
    }
    Ok(report)
}

/// Audio extensions that take part in the folder consensus.
const AUDIO_EXT: &[&str] = &[
    "mp3", "flac", "m4a", "mp4", "ogg", "oga", "opus", "wav", "aif", "aiff", "wma", "ape", "wv",
];

fn mode(values: impl Iterator<Item = String>) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for v in values {
        let key = sirtone_core::text::norm(&v).unwrap_or_default();
        match counts
            .iter_mut()
            .find(|(k, _)| sirtone_core::text::norm(k).unwrap_or_default() == key)
        {
            Some((_, n)) => *n += 1,
            None => counts.push((v, 1)),
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v)
}

/// Parent/grandparent folder names of the source and the consensus of tags in its folder
/// (≥ 3 audio files: the most common album / album artist).
fn folder_hints(
    src: &Path,
    tags: &dyn TagReader,
    cache: &mut std::collections::HashMap<PathBuf, (Option<String>, Option<String>)>,
) -> FolderHints {
    let name = |p: Option<&Path>| {
        p.and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    };
    let parent = src.parent();
    let (consensus_album, consensus_album_artist) = match parent {
        Some(dir) => cache
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                let files: Vec<PathBuf> = fs::read_dir(dir)
                    .map(|rd| {
                        rd.flatten()
                            .map(|e| e.path())
                            .filter(|p| AUDIO_EXT.contains(&ext_of(p).as_str()))
                            .take(200)
                            .collect()
                    })
                    .unwrap_or_default();
                if files.len() < 3 {
                    return (None, None);
                }
                let read: Vec<_> = files.iter().map(|f| tags.read(f)).collect();
                (
                    mode(read.iter().filter_map(|t| t.album.clone())),
                    mode(read.iter().filter_map(|t| t.album_artist.clone())),
                )
            })
            .clone(),
        None => (None, None),
    };
    FolderHints {
        parent: name(parent),
        grandparent: name(parent.and_then(Path::parent)),
        consensus_album,
        consensus_album_artist,
    }
}

#[allow(clippy::too_many_arguments)]
fn place(
    conn: &mut Connection,
    root: &Path,
    batch: i64,
    item: i64,
    staged: &Path,
    hash: &str,
    info: &MediaInfo,
    src: &Path,
    hints: &FolderHints,
    deps: &ImportDeps,
    report: &mut Report,
    on_event: &mut dyn FnMut(ImportEvent),
) -> Result<(), LibraryError> {
    // Exact copy already in the library → duplicate (docs/05 §6), the staged copy is ours to drop.
    if repo::track_by_hash(conn, hash)?.is_some() {
        let _ = fs::remove_file(staged);
        set_state(conn, item, "duplicate", Some("exact_copy"))?;
        return Ok(());
    }
    let tags = deps.tags.read(staged);
    let file_name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let d = deps.identify.identify(&tags, &file_name, hints, info);
    let ext = ext_of(src);
    let primary = d.artists.first().cloned();
    let placement = match (&d.unknown, primary.as_deref()) {
        (None, Some(artist)) => layout::canonical(&TrackPlace {
            artist,
            album_artist: d.album_artist.as_deref(),
            album: d.album.as_deref(),
            title: &d.title,
            track_no: d.track_no,
            disc_no: d.disc_no,
            multi_disc: d.disc_total.is_some_and(|n| n > 1) || d.disc_no.is_some_and(|n| n > 1),
            ext: &ext,
        }),
        _ => layout::unknown(&file_name),
    };
    let rel = layout::deduplicate(&placement.rel_path, |c| {
        from_rel(root, c).exists()
            || conn
                .query_row(
                    "SELECT 1 FROM track WHERE rel_path = ?1 COLLATE NOCASE",
                    [c],
                    |_| Ok(()),
                )
                .optional()
                .ok()
                .flatten()
                .is_some()
    });
    if rel.chars().count() > MAX_REL_PATH + 8 {
        let _ = fs::remove_file(staged);
        set_state(conn, item, "rejected", Some(Rejected::PathTooLong.as_str()))?;
        return Ok(());
    }
    let dst = from_rel(root, &rel);
    if let Some(dir) = dst.parent() {
        fs::create_dir_all(dir)?;
    }
    // Journal → durable move → one transaction for track + journal + item (docs/03 §8.3).
    conn.execute(
        "INSERT INTO fs_journal(op, src, dst, state, ts) VALUES ('move', ?1, ?2, 'planned', ?3)",
        params![to_rel(root, staged), rel, now_ms()],
    )?;
    let journal = conn.last_insert_rowid();
    if let Err(e) = deps.fs.move_durable(staged, &dst) {
        conn.execute(
            "UPDATE fs_journal SET state = 'rolled_back' WHERE id = ?1",
            [journal],
        )?;
        return Err(e.into());
    }
    let size = fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
    let mtime = fs::metadata(&dst)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let tx = conn.transaction()?;
    let artists = d
        .artists
        .iter()
        .map(|a| repo::artist_get_or_create(&tx, a))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default();
    let album_id = match (&d.album, d.unknown) {
        (Some(album), None) => {
            let aa = d
                .album_artist
                .as_deref()
                .or(d.artists.first().map(String::as_str));
            match aa.map(|a| repo::artist_get_or_create(&tx, a)) {
                Some(Ok(aa)) => repo::album_get_or_create(&tx, album, aa, d.year).ok(),
                _ => None,
            }
        }
        _ => None,
    };
    let track = NewTrack {
        rel_path: rel.clone(),
        size_bytes: size,
        mtime_ms: mtime,
        content_hash: hash.to_string(),
        container: info.container.clone(),
        codec: info.codec.clone(),
        lossless: info.lossless,
        sample_rate: info.sample_rate,
        channels: info.channels,
        duration_ms: info.duration_ms,
        title: d.title.clone(),
        album_id,
        disc_no: d.disc_no,
        track_no: d.track_no,
        year: d.year,
        identity: if d.unknown.is_some() {
            Identity::Unknown
        } else {
            Identity::Inferred
        },
        identity_conf: d.confidence,
        unknown_reason: d.unknown.map(|r| r.as_str().to_string()),
        artists,
    };
    let id = repo::track_insert_in(&tx, &track)?;
    tx.execute(
        "UPDATE track SET import_batch_id = ?2, genre_main = ?3 WHERE id = ?1",
        params![id, batch, d.genre],
    )?;
    tx.execute(
        "UPDATE fs_journal SET state = 'done' WHERE id = ?1",
        [journal],
    )?;
    tx.execute(
        "UPDATE import_item SET state = 'placed', track_id = ?2 WHERE id = ?1",
        params![item, id],
    )?;
    tx.commit()?;
    if d.genre.is_some() {
        repo::search_reindex(conn, id)?;
    }
    // Album cover (FR-REC-009, offline part): first track that brings one wins. Best effort —
    // a broken picture never fails the import. (docs/03 §10 `probe_cover`; inline until the
    // job queue exists.)
    if let Some(album) = album_id
        && !crate::covers::album_has_cover(conn, album).unwrap_or(true)
        && let Some((bytes, source)) = deps.covers.read(&dst, src.parent())
    {
        match crate::covers::store(conn, root, &bytes, source) {
            Ok(cover) => {
                let _ = crate::covers::link_album(conn, album, cover);
            }
            Err(e) => tracing::warn!(target: "sirtone::import", error = %e, "cover skipped"),
        }
    }
    report.added += 1;
    on_event(ImportEvent::TrackAdded {
        track_id: id,
        unknown: d.unknown.is_some(),
    });
    Ok(())
}

/// Startup recovery (docs/03 §8.3, docs/05 §3.3): settles every `planned` journal row by what is
/// actually on disk, drops `.part` leftovers in staging. Never deletes anything outside staging.
pub fn recover(
    conn: &mut Connection,
    root: &Path,
    fsops: &dyn FileOps,
) -> Result<u32, LibraryError> {
    let mut fixed = 0;
    let rows: Vec<(i64, String, String)> = {
        let mut st = conn.prepare(
            "SELECT id, src, dst FROM fs_journal WHERE state = 'planned' AND op = 'move'",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?
    };
    let quarantine = root.join(SERVICE_DIR).join("quarantine");
    for (id, src_rel, dst_rel) in rows {
        let (src, dst) = (from_rel(root, &src_rel), from_rel(root, &dst_rel));
        let indexed = conn
            .query_row(
                "SELECT 1 FROM track WHERE rel_path = ?1",
                [&dst_rel],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        match (src.exists(), dst.exists()) {
            (true, false) => {}
            (false, true) if !indexed => {
                // The move happened, the transaction did not: put the copy back into staging.
                fsops.move_durable(&dst, &src)?;
            }
            (false, true) => {} // indexed: the transaction did commit; only the journal lagged
            (true, true) | (false, false) => {
                if dst.exists() && !indexed {
                    fs::create_dir_all(&quarantine)?;
                    let q = quarantine.join(format!(
                        "{id}-{}",
                        dst.file_name()
                            .map(|n| n.to_string_lossy())
                            .unwrap_or_default()
                    ));
                    fsops.move_durable(&dst, &q)?;
                }
                tracing::warn!(target: "sirtone::import", journal = id, "journal row settled by quarantine");
            }
        }
        let state = if indexed { "done" } else { "rolled_back" };
        conn.execute(
            "UPDATE fs_journal SET state = ?2 WHERE id = ?1",
            params![id, state],
        )?;
        fixed += 1;
    }
    // `.part` leftovers are ours (staging only).
    let staging = root.join(SERVICE_DIR).join("staging");
    if let Ok(batches) = fs::read_dir(&staging) {
        for b in batches.flatten() {
            if let Ok(files) = fs::read_dir(b.path()) {
                for f in files.flatten() {
                    if f.file_name().to_string_lossy().ends_with(".part") {
                        let _ = fs::remove_file(f.path());
                    }
                }
            }
        }
    }
    Ok(fixed)
}
