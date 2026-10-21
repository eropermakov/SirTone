//! Export (T5.9, FR-DATA-003, `05` §12, AT-09; `01b` F-33): the whole library, a playlist, an
//! album or an artist into a chosen folder as ordinary files in their own format — copied byte
//! for byte, the originals untouched. Options: keep `Артист/Альбом`, number the files in the
//! list's order (`01 - …`, for car stereos). Free space is checked first; progress and cancel.
//! A file is written as `*.partial` and renamed when complete, so a cancelled or failed copy
//! never leaves a half file under a real name (the partial — our own temp — is removed). An
//! existing file of the same size is taken as already exported; another one with that name
//! gets « (2)».

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::ports::VolumeInfo;
use sirtone_library::{collection, playlists};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::library::Library;

static BUSY: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// What to export and how.
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportRequest {
    /// `all` · `playlist` · `album` · `artist`
    pub kind: String,
    pub id: Option<u32>,
    pub dest: String,
    /// `Артист/Альбом/файл` as in the library; off — all files in one folder.
    pub keep_structure: bool,
    /// `01 - …` in the list's order.
    pub number: bool,
}

/// What `export_start` is about to copy.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlan {
    pub files: u32,
    pub bytes: f64,
}

/// Progress of the running export (a few times a second and at the end).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct ExportProgress {
    pub done: u32,
    pub total: u32,
    pub bytes: f64,
    pub total_bytes: f64,
    /// Already there with the same size.
    pub skipped: u32,
    pub finished: bool,
    pub cancelled: bool,
    /// The first error that stopped it (disk full, no access).
    pub error: Option<AppError>,
}

struct Item {
    src: PathBuf,
    rel: PathBuf,
    size: u64,
}

fn ids(lib: &Library, req: &ExportRequest) -> Result<Vec<i64>, AppError> {
    let id = || {
        req.id
            .map(i64::from)
            .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument).with_details("id"))
    };
    match req.kind.as_str() {
        "all" => lib.with_read(|c| {
            let mut st = c.prepare("SELECT id FROM track WHERE state = 'ok' ORDER BY rel_path")?;
            let v = st
                .query_map([], |r| r.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(v)
        }),
        "playlist" => {
            let p = id()?;
            lib.with_read(|c| {
                Ok(playlists::tracks(c, p)?
                    .into_iter()
                    .map(|t| t.track.id)
                    .collect())
            })
        }
        "album" => {
            let a = id()?;
            lib.with_read(|c| {
                Ok(collection::album_tracks(c, a)?
                    .into_iter()
                    .map(|t| t.id)
                    .collect())
            })
        }
        "artist" => {
            let a = id()?;
            lib.with_read(|c| {
                Ok(collection::artist_tracks(c, a)?
                    .into_iter()
                    .map(|t| t.id)
                    .collect())
            })
        }
        k => Err(AppError::new(ErrorCode::InvalidArgument).with_details(k.to_owned())),
    }
}

fn items(lib: &Library, ids: &[i64]) -> Result<Vec<Item>, AppError> {
    let root = lib
        .root()
        .ok_or_else(|| AppError::new(ErrorCode::LibraryRootNotSet))?;
    let rels: Vec<String> = lib.with_read(|c| {
        let mut st = c.prepare_cached("SELECT rel_path FROM track WHERE id = ?1")?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok(r) = st.query_row([id], |r| r.get::<_, String>(0)) {
                out.push(r);
            }
        }
        Ok(out)
    })?;
    let mut seen = std::collections::HashSet::new();
    Ok(rels
        .into_iter()
        .filter(|r| seen.insert(r.clone()))
        .filter_map(|rel| {
            let src = root.join(&rel);
            let size = std::fs::metadata(&src).ok()?.len();
            Some(Item {
                src,
                rel: PathBuf::from(rel),
                size,
            })
        })
        .collect())
}

/// Where the `n`-th of `total` files goes.
fn target(dest: &Path, it: &Item, n: usize, total: usize, req: &ExportRequest) -> PathBuf {
    let name = it.rel.file_name().map(|f| f.to_string_lossy().into_owned());
    let name = name.unwrap_or_default();
    let name = if req.number {
        let w = total.to_string().len().max(2);
        // The library's own «07 - » prefix gives way to the list's order.
        format!("{:0w$} - {}", n + 1, strip_number(&name))
    } else {
        name
    };
    match it.rel.parent().filter(|_| req.keep_structure) {
        Some(p) => dest.join(p).join(name),
        None => dest.join(name),
    }
}

/// `07 - Song.flac` → `Song.flac` (the layout's track prefix, `05` §4).
fn strip_number(name: &str) -> &str {
    let digits = name.bytes().take_while(u8::is_ascii_digit).count();
    match name[digits..].strip_prefix(" - ") {
        Some(rest) if (1..=3).contains(&digits) && !rest.is_empty() => rest,
        _ => name,
    }
}

/// A free name: `x.flac` → `x (2).flac` …
fn free_name(p: &Path) -> PathBuf {
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2..)
        .map(|k| p.with_file_name(format!("{stem} ({k}){ext}")))
        .find(|c| !c.exists())
        .unwrap_or_else(|| p.to_path_buf())
}

fn io_err(e: &std::io::Error) -> AppError {
    let code = match e.raw_os_error() {
        Some(112) => ErrorCode::DiskFull,
        Some(5) => ErrorCode::PermissionDenied,
        _ => ErrorCode::Internal,
    };
    AppError::new(code).with_details(e.to_string())
}

fn copy_one(src: &Path, to: &Path) -> Result<(), AppError> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| io_err(&e))?;
    }
    let mut part = to.as_os_str().to_owned();
    part.push(".partial");
    let part = PathBuf::from(part);
    let r = std::fs::copy(src, &part).and_then(|_| std::fs::rename(&part, to));
    if let Err(e) = r {
        let _ = std::fs::remove_file(&part); // our own temp, never the person's file
        return Err(io_err(&e));
    }
    Ok(())
}

fn run(app: &AppHandle, items: &[Item], req: &ExportRequest) -> ExportProgress {
    let dest = PathBuf::from(&req.dest);
    let total_bytes: u64 = items.iter().map(|i| i.size).sum();
    let mut p = ExportProgress {
        done: 0,
        total: items.len() as u32,
        bytes: 0.0,
        total_bytes: total_bytes as f64,
        skipped: 0,
        finished: false,
        cancelled: false,
        error: None,
    };
    let mut last = std::time::Instant::now();
    for (n, it) in items.iter().enumerate() {
        if CANCEL.load(Ordering::Relaxed) {
            p.cancelled = true;
            break;
        }
        let mut to = target(&dest, it, n, items.len(), req);
        let same = std::fs::metadata(&to).is_ok_and(|m| m.len() == it.size);
        if same {
            p.skipped += 1;
        } else {
            if to.exists() {
                to = free_name(&to);
            }
            if let Err(e) = copy_one(&it.src, &to) {
                p.error = Some(e);
                break;
            }
        }
        p.done += 1;
        p.bytes += it.size as f64;
        if last.elapsed().as_millis() >= 250 {
            last = std::time::Instant::now();
            let _ = p.clone().emit(app);
        }
    }
    p.finished = true;
    p
}

/// Plans and starts the export on its own thread; progress comes as `ExportProgress`.
#[tauri::command(async)]
#[specta::specta]
pub fn export_start(
    app: AppHandle,
    lib: State<'_, Library>,
    req: ExportRequest,
) -> Result<ExportPlan, AppError> {
    let dest = PathBuf::from(&req.dest);
    if !dest.is_absolute() {
        return Err(AppError::new(ErrorCode::InvalidArgument).with_details("dest"));
    }
    // Never into the library itself.
    if let Some(root) = lib.root()
        && dest.starts_with(&root)
    {
        return Err(AppError::new(ErrorCode::InvalidArgument).with_details("inside the library"));
    }
    let ids = ids(&lib, &req)?;
    let items = items(&lib, &ids)?;
    let bytes: u64 = items.iter().map(|i| i.size).sum();
    std::fs::create_dir_all(&dest).map_err(|e| io_err(&e))?;
    if let Some(free) = sirtone_platform::files::WinVolume.free_bytes(&dest)
        && free < bytes
    {
        return Err(AppError::new(ErrorCode::DiskFull).with_details(format!("{bytes} > {free}")));
    }
    if BUSY.swap(true, Ordering::AcqRel) {
        return Err(AppError::new(ErrorCode::DeviceBusy).with_details("an export runs"));
    }
    CANCEL.store(false, Ordering::Relaxed);
    let plan = ExportPlan {
        files: items.len() as u32,
        bytes: bytes as f64,
    };
    let spawned = std::thread::Builder::new()
        .name("sirtone-export".into())
        .spawn(move || {
            let p = run(&app, &items, &req);
            tracing::info!(target: "sirtone::export", done = p.done, skipped = p.skipped, cancelled = p.cancelled, failed = p.error.is_some(), "export finished");
            BUSY.store(false, Ordering::Release);
            let _ = p.emit(&app);
        });
    if spawned.is_err() {
        BUSY.store(false, Ordering::Release);
        return Err(AppError::new(ErrorCode::Internal));
    }
    Ok(plan)
}

#[tauri::command]
#[specta::specta]
pub fn export_cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(keep: bool, number: bool) -> ExportRequest {
        ExportRequest {
            kind: "playlist".into(),
            id: Some(1),
            dest: "D:/out".into(),
            keep_structure: keep,
            number,
        }
    }

    #[test]
    fn targets_keep_the_structure_and_number_in_order() {
        let it = Item {
            src: PathBuf::from("D:/lib/A/B/song.flac"),
            rel: PathBuf::from("A/B/song.flac"),
            size: 1,
        };
        let dest = Path::new("D:/out");
        assert_eq!(
            target(dest, &it, 0, 12, &req(true, false)),
            PathBuf::from("D:/out/A/B/song.flac")
        );
        assert_eq!(
            target(dest, &it, 2, 12, &req(false, true)),
            PathBuf::from("D:/out/03 - song.flac")
        );
        assert_eq!(
            target(dest, &it, 6, 150, &req(false, true)),
            PathBuf::from("D:/out/007 - song.flac")
        );
        let numbered = Item {
            src: PathBuf::new(),
            rel: PathBuf::from("A/B/07 - song.flac"),
            size: 1,
        };
        assert_eq!(
            target(dest, &numbered, 0, 3, &req(false, true)),
            PathBuf::from("D:/out/01 - song.flac")
        );
        assert_eq!(strip_number("2024 - x.flac"), "2024 - x.flac");
    }

    #[test]
    fn a_copy_is_byte_for_byte_and_a_taken_name_gets_a_number() {
        let dir = std::env::temp_dir().join(format!("sirtone-export-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src.bin");
        let data: Vec<u8> = (0..70_000u32).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::write(&src, &data).unwrap();
        let to = dir.join("out").join("x.bin");
        copy_one(&src, &to).unwrap();
        assert_eq!(std::fs::read(&to).unwrap(), data);
        assert!(!dir.join("out").join("x.bin.partial").exists());
        assert_eq!(free_name(&to), dir.join("out").join("x (2).bin"));
    }
}
