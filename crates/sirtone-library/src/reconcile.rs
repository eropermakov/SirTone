//! Disk ↔ database reconcile (docs/05 §7, T1.19–T1.20; copy of Navidrome/Plex scanning).
//!
//! One pass: walk the root, then for every track decide present / moved / missing, adopt audio
//! files that appeared in place (Explorer), and purge rows missing for 24 h. Safety rules:
//! an unreadable root is «offline» and changes nothing; a folder that failed to enumerate marks
//! nothing under it; a large disappearance (> 20 % or > 200 files) becomes a question instead of
//! marks. Moves keep the row (and with it statistics, journal, playlists).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::LibraryError;
use crate::import::{
    AUDIO_EXT, FileFacts, ImportDeps, SERVICE_DIR, attach_cover, ext_of, folder_hints, hash_file,
    insert_identified,
};
use crate::repo::{self, TrackId};

/// A missing row is purged after this long (FR-FS-003: «тихо очищается через 24 ч»).
pub const PURGE_AFTER_MS: i64 = 24 * 3600 * 1000;
/// More than this many files gone in one pass is a question, not a mark (docs/05 §7).
pub const MASS_FILES: usize = 200;
pub const MASS_SHARE: f64 = 0.20;

#[derive(Debug, Clone)]
pub struct DiskFile {
    pub rel: String,
    pub size: u64,
    pub mtime_ms: i64,
}

#[derive(Debug, Default)]
pub struct Scan {
    pub files: Vec<DiskFile>,
    /// Folders (relative, `/`-separated) that could not be listed: nothing under them is
    /// considered gone.
    pub failed_dirs: Vec<String>,
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn abs_of(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c))
}

fn mtime_ms(m: &fs::Metadata) -> i64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as i64)
}

/// Folders a scan never enters: our service folder and Windows system folders at a drive root.
fn skipped_dir(name: &str) -> bool {
    name == SERVICE_DIR
        || name.eq_ignore_ascii_case("System Volume Information")
        || name.eq_ignore_ascii_case("$RECYCLE.BIN")
}

/// Walks the root (`jwalk`, parallel). `Err` when the root itself cannot be read: offline.
pub fn scan(root: &Path) -> Result<Scan, std::io::Error> {
    fs::read_dir(root)?; // the root must be listable, or the library is offline
    let mut out = Scan::default();
    let walk = jwalk::WalkDir::new(root)
        .skip_hidden(false)
        .process_read_dir(|_, _, _, children| {
            children.retain(|e| match e {
                Ok(e) => {
                    let name = e.file_name().to_string_lossy();
                    !(e.file_type().is_dir() && skipped_dir(&name)) && !name.ends_with(".part")
                }
                Err(_) => true,
            });
        });
    for entry in walk {
        match entry {
            Ok(e) if e.file_type().is_file() => {
                let path = e.path();
                if !AUDIO_EXT.contains(&ext_of(&path).as_str()) {
                    continue;
                }
                match e.metadata() {
                    Ok(m) => out.files.push(DiskFile {
                        rel: rel_of(root, &path),
                        size: m.len(),
                        mtime_ms: mtime_ms(&m),
                    }),
                    Err(_) => out.failed_dirs.push(rel_of(root, &path)),
                }
            }
            Ok(_) => {}
            Err(err) => {
                // A folder we could not list: protect everything under it.
                let dir = err.path().map_or_else(String::new, |p| rel_of(root, p));
                out.failed_dirs.push(dir);
            }
        }
    }
    if out.failed_dirs.iter().any(String::is_empty) {
        // The root failed mid-walk (stick pulled): treat as offline.
        return Err(std::io::Error::other("root became unreadable"));
    }
    Ok(out)
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Report {
    /// Files added in Explorer and taken into the library where they are.
    pub adopted: usize,
    /// Their tracks (T3.19: «навести порядок» is offered for those not where the rules say).
    pub adopted_ids: Vec<TrackId>,
    pub moved: usize,
    pub missing_marked: usize,
    /// Missing tracks whose file is back.
    pub returned: usize,
    /// Size/mtime refreshed (the file was edited in place).
    pub updated: usize,
    pub purged: usize,
    /// Not audio / broken / exact copies, remembered in `fs_ignore`.
    pub ignored: usize,
    pub file_ids_filled: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The root cannot be read: nothing was touched (FR-FS-004).
    Offline,
    Done(Report),
    /// Many files vanished at once: moves and additions were applied, nothing was marked or
    /// purged; the user decides (FR-FS-003). `confirm_missing` marks them.
    Question {
        gone: usize,
        report: Report,
    },
}

struct Row {
    id: i64,
    rel: String,
    size: u64,
    mtime: i64,
    file_id: Option<String>,
    hash: String,
    missing: bool,
    missing_since: Option<i64>,
}

fn rows(conn: &Connection) -> Result<Vec<Row>, LibraryError> {
    let mut st = conn.prepare(
        "SELECT id, rel_path, size_bytes, mtime_ms, file_id, content_hash, state, missing_since
         FROM track",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Row {
            id: r.get(0)?,
            rel: r.get(1)?,
            size: r.get::<_, i64>(2)?.max(0) as u64,
            mtime: r.get(3)?,
            file_id: r.get(4)?,
            hash: r.get(5)?,
            missing: r.get::<_, String>(6)? == "missing",
            missing_since: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn under(rel: &str, dirs: &[String]) -> bool {
    dirs.iter()
        .any(|d| rel == d || rel.starts_with(&format!("{d}/")) || d.is_empty())
}

/// One reconcile pass. `mass_confirmed` = the user answered «yes, they are gone» to the last
/// question. `now_ms` is injected for tests.
pub fn reconcile(
    conn: &mut Connection,
    root: &Path,
    deps: &ImportDeps,
    now_ms: i64,
    mass_confirmed: bool,
) -> Result<Outcome, LibraryError> {
    let Ok(scan) = scan(root) else {
        return Ok(Outcome::Offline);
    };
    let mut report = Report::default();
    let db = rows(conn)?;
    let on_disk: HashMap<&str, &DiskFile> =
        scan.files.iter().map(|f| (f.rel.as_str(), f)).collect();
    let known: HashSet<&str> = db.iter().map(|r| r.rel.as_str()).collect();

    // 1. Present files: back from missing, edited in place, file id backfill.
    for r in &db {
        let Some(f) = on_disk.get(r.rel.as_str()) else {
            continue;
        };
        if r.missing {
            conn.execute(
                "UPDATE track SET state = 'ok', missing_since = NULL WHERE id = ?1",
                [r.id],
            )?;
            report.returned += 1;
        }
        if f.size != r.size || f.mtime_ms != r.mtime {
            conn.execute(
                "UPDATE track SET size_bytes = ?2, mtime_ms = ?3 WHERE id = ?1",
                params![r.id, f.size as i64, f.mtime_ms],
            )?;
            report.updated += 1;
        }
        if r.file_id.is_none()
            && let Some(id) = deps.fs.file_id(&abs_of(root, &r.rel))
        {
            conn.execute(
                "UPDATE track SET file_id = ?2 WHERE id = ?1",
                params![r.id, id],
            )?;
            report.file_ids_filled += 1;
        }
    }

    // 2. New files on disk (not tracks, not remembered as ignorable).
    let ignored: HashMap<String, (u64, i64)> = {
        let mut st = conn.prepare("SELECT rel_path, size_bytes, mtime_ms FROM fs_ignore")?;
        st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                (r.get::<_, i64>(1)?.max(0) as u64, r.get(2)?),
            ))
        })?
        .collect::<Result<_, _>>()?
    };
    let new: Vec<&DiskFile> = scan
        .files
        .iter()
        .filter(|f| !known.contains(f.rel.as_str()))
        .filter(|f| ignored.get(&f.rel) != Some(&(f.size, f.mtime_ms)))
        .collect();

    // 3. Candidates for a move: gone now (outside failed folders) or already missing.
    let mut candidates: Vec<&Row> = db
        .iter()
        .filter(|r| !on_disk.contains_key(r.rel.as_str()) && !under(&r.rel, &scan.failed_dirs))
        .collect();
    let mut adopt: Vec<&DiskFile> = Vec::new();
    for f in new {
        let abs = abs_of(root, &f.rel);
        let fid = deps.fs.file_id(&abs);
        let by_id = fid.as_deref().and_then(|id| {
            candidates
                .iter()
                .position(|r| r.file_id.as_deref() == Some(id))
        });
        let found = match by_id {
            Some(i) => Some(i),
            // FAT/exFAT or no id recorded: same size, then the full hash decides (docs/05 §7).
            None if candidates.iter().any(|r| r.size == f.size) => match hash_file(&abs) {
                Ok(h) => candidates
                    .iter()
                    .position(|r| r.size == f.size && r.hash == h),
                Err(_) => None,
            },
            None => None,
        };
        match found {
            Some(i) => {
                let r = candidates.swap_remove(i);
                conn.execute(
                    "UPDATE track SET rel_path = ?2, size_bytes = ?3, mtime_ms = ?4,
                            file_id = coalesce(?5, file_id), state = 'ok', missing_since = NULL
                     WHERE id = ?1",
                    params![r.id, f.rel, f.size as i64, f.mtime_ms, fid],
                )?;
                report.moved += 1;
            }
            None => adopt.push(f),
        }
    }

    // 4. Adopt the rest in place (FR-FS-005 core, pulled forward from T3.19).
    let mut hints_cache = HashMap::new();
    for f in adopt {
        match adopt_in_place(conn, root, f, deps, &mut hints_cache)? {
            Some(id) => {
                report.adopted += 1;
                report.adopted_ids.push(id);
            }
            None => report.ignored += 1,
        }
    }

    // 5. What is gone for real: mark missing, unless it is a mass disappearance.
    let gone: Vec<&Row> = candidates.iter().filter(|r| !r.missing).copied().collect();
    let total_ok = db.iter().filter(|r| !r.missing).count();
    let mass = gone.len() > MASS_FILES
        || (total_ok > 0 && gone.len() as f64 > MASS_SHARE * total_ok as f64 && gone.len() > 1);
    if mass && !mass_confirmed {
        return Ok(Outcome::Question {
            gone: gone.len(),
            report,
        });
    }
    for r in &gone {
        conn.execute(
            "UPDATE track SET state = 'missing', missing_since = ?2 WHERE id = ?1",
            params![r.id, now_ms],
        )?;
        report.missing_marked += 1;
    }

    // 6. Purge rows missing for 24 h whose file is still absent (FR-FS-003).
    for r in db.iter().filter(|r| r.missing) {
        if r.missing_since
            .is_some_and(|t| now_ms - t >= PURGE_AFTER_MS)
            && !abs_of(root, &r.rel).exists()
            && candidates.iter().any(|c| c.id == r.id)
        {
            repo::track_remove(conn, r.id)?;
            report.purged += 1;
        }
    }
    Ok(Outcome::Done(report))
}

fn ignore(conn: &Connection, f: &DiskFile, reason: &str, now: i64) -> Result<(), LibraryError> {
    conn.execute(
        "INSERT INTO fs_ignore(rel_path, size_bytes, mtime_ms, reason, ts) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(rel_path) DO UPDATE SET size_bytes = excluded.size_bytes,
           mtime_ms = excluded.mtime_ms, reason = excluded.reason, ts = excluded.ts",
        params![f.rel, f.size as i64, f.mtime_ms, reason, now],
    )?;
    Ok(())
}

/// A file that appeared inside the root: probe, hash, identify and insert it where it is.
/// `false` = not a track (remembered in `fs_ignore`).
fn adopt_in_place(
    conn: &mut Connection,
    root: &Path,
    f: &DiskFile,
    deps: &ImportDeps,
    hints_cache: &mut HashMap<PathBuf, (Option<String>, Option<String>)>,
) -> Result<Option<TrackId>, LibraryError> {
    let abs = abs_of(root, &f.rel);
    let now = crate::db::now_ms();
    let info = match deps.probe.probe(&abs) {
        Ok(i) => i,
        Err(_) => {
            ignore(conn, f, "not_playable", now)?;
            return Ok(None);
        }
    };
    let Ok(hash) = hash_file(&abs) else {
        return Ok(None); // still being written: next pass
    };
    if repo::track_by_hash(conn, &hash)?.is_some() {
        ignore(conn, f, "exact_copy", now)?;
        return Ok(None);
    }
    let tags = deps.tags.read(&abs);
    let file_name = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let hints = folder_hints(&abs, deps.tags.as_ref(), hints_cache);
    let d = deps.identify.identify(&tags, &file_name, &hints, &info);
    let fid = deps.fs.file_id(&abs);
    let tx = conn.transaction()?;
    let (id, album) = insert_identified(
        &tx,
        &FileFacts {
            rel: &f.rel,
            size: f.size,
            mtime: f.mtime_ms,
            hash: &hash,
            file_id: fid.as_deref(),
        },
        &info,
        &d,
    )?;
    tx.execute("DELETE FROM fs_ignore WHERE rel_path = ?1", [&f.rel])?;
    tx.commit()?;
    if d.genre.is_some() {
        repo::search_reindex(conn, id)?;
    }
    attach_cover(conn, root, album, &abs, abs.parent(), deps);
    Ok(Some(id))
}

/// Whether any track is missing right now (for the UI and the purge guard).
pub fn missing_count(conn: &Connection) -> Result<u64, LibraryError> {
    Ok(conn
        .query_row(
            "SELECT count(*) FROM track WHERE state = 'missing'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0)
        .max(0) as u64)
}
