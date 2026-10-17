//! `sirtone-cli import` / `verify`: the import pipeline with the real ports, without UI.
//! Used for P-10 (files/s) and the kill-during-import harness (`tools/import-kill-harness.py`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use sirtone_core::ports::VolumeInfo;
use sirtone_library::import::{self, ImportDeps, ImportEvent, SERVICE_DIR};
use sirtone_library::{Medium, collection};

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

fn medium(root: &Path) -> Medium {
    if sirtone_platform::files::WinVolume.is_removable(root) {
        Medium::Removable
    } else {
        Medium::Fixed
    }
}

/// `import` creates the library when it is not there yet.
fn open(root: &Path) -> Result<rusqlite::Connection, String> {
    sirtone_library::open(&root.join(SERVICE_DIR).join("library.db"), medium(root))
        .map_err(|e| e.to_string())
}

/// `verify` only reads an existing library: it never creates one in an arbitrary folder.
fn open_existing(root: &Path) -> Result<rusqlite::Connection, String> {
    sirtone_library::open_existing(&root.join(SERVICE_DIR).join("library.db"), medium(root))
        .map_err(|e| e.to_string())
}

/// `import <library_root> [sources...]` — recovers, resumes unfinished batches, then imports
/// `sources` as a new batch. With no sources it only recovers and resumes.
pub fn import(root: &str, sources: &[String]) -> ExitCode {
    let root = PathBuf::from(root);
    let deps = deps();
    let mut db = match open(&root) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("open: {e}");
            return ExitCode::FAILURE;
        }
    };
    match import::recover(&mut db, &root, deps.fs.as_ref()) {
        Ok(n) if n > 0 => println!("recovered {n} journal rows"),
        Ok(_) => {}
        Err(e) => {
            eprintln!("recover: {e}");
            return ExitCode::FAILURE;
        }
    }
    let mut batches = import::unfinished_batches(&db).unwrap_or_default();
    if !sources.is_empty() {
        let paths: Vec<PathBuf> = sources.iter().map(PathBuf::from).collect();
        match import::start_batch(&mut db, &paths, "external") {
            Ok(b) => batches.push(b),
            Err(e) => {
                eprintln!("start: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let cancel = AtomicBool::new(false);
    for batch in batches {
        let t0 = Instant::now();
        let mut released_at = None;
        let mut on_event = |e: ImportEvent| match e {
            ImportEvent::Progress { done, total } if done % 50 == 0 || done == total => {
                println!("batch {batch}: {done}/{total}");
            }
            ImportEvent::SourcesReleased => released_at = Some(t0.elapsed().as_secs_f64()),
            _ => {}
        };
        match import::run_batch(&mut db, &root, batch, &deps, &cancel, &mut on_event) {
            Ok(r) => {
                let secs = t0.elapsed().as_secs_f64();
                let processed = r.added + r.duplicates.len() as u64 + r.rejected.len() as u64;
                println!(
                    "batch {batch}: added {} (unknown {}), duplicates {}, rejected {}, {:.1} s, {:.1} files/s{}",
                    r.added,
                    r.unknown,
                    r.duplicates.len(),
                    r.rejected.len(),
                    secs,
                    processed as f64 / secs.max(1e-9),
                    released_at
                        .map(|s| format!(", medium free after {s:.1} s"))
                        .unwrap_or_default()
                );
                let mut reasons: Vec<&str> = r.rejected.iter().map(|(_, r)| r.as_str()).collect();
                reasons.sort_unstable();
                reasons.dedup();
                if !reasons.is_empty() {
                    println!("  rejected reasons: {}", reasons.join(", "));
                }
            }
            Err(e) => {
                eprintln!("batch {batch}: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

/// `verify <library_root>` — database and disk agree: every track file exists with its hash, no
/// library file is unindexed, no journal row is left `planned`, staging holds no files.
pub fn verify(root: &str) -> ExitCode {
    let root = PathBuf::from(root);
    let db = match open_existing(&root) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("open: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut problems = Vec::new();
    let mut st = match db.prepare("SELECT rel_path, content_hash FROM track") {
        Ok(st) => st,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let rows: Vec<(String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map(|it| it.filter_map(Result::ok).collect())
        .unwrap_or_default();
    let mut indexed = HashSet::new();
    for (rel, hash) in &rows {
        let p = rel.split('/').fold(root.clone(), |p, c| p.join(c));
        match import::hash_file(&p) {
            Ok(h) if &h == hash => {}
            Ok(_) => problems.push(format!("hash mismatch: {rel}")),
            Err(_) => problems.push(format!("missing on disk: {rel}")),
        }
        indexed.insert(rel.to_lowercase());
    }
    let mut on_disk = Vec::new();
    walk(&root, &root, &mut on_disk);
    for rel in &on_disk {
        if !indexed.contains(&rel.to_lowercase()) {
            problems.push(format!("not indexed: {rel}"));
        }
    }
    let planned: i64 = db
        .query_row(
            "SELECT count(*) FROM fs_journal WHERE state = 'planned'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(-1);
    if planned != 0 {
        problems.push(format!("journal rows still planned: {planned}"));
    }
    let mut staged = Vec::new();
    walk(&root.join(SERVICE_DIR).join("staging"), &root, &mut staged);
    let open_batches = import::unfinished_batches(&db).unwrap_or_default();
    if !staged.is_empty() && open_batches.is_empty() {
        problems.push(format!("files left in staging: {}", staged.len()));
    }
    let n = collection::counts(&db).unwrap_or_default();
    println!(
        "verify: {} tracks ({} unknown), {} artists, {} albums, {} files on disk, {} problems",
        n.tracks,
        n.unknown,
        n.artists,
        n.albums,
        on_disk.len(),
        problems.len()
    );
    for p in problems.iter().take(20) {
        println!("  {p}");
    }
    if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Library files (relative, `/`), skipping the service folder.
fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.file_name().is_some_and(|n| n == SERVICE_DIR) && dir == root {
            continue;
        }
        if p.is_dir() {
            walk(&p, root, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(
                rel.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
            );
        }
    }
}
