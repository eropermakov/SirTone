//! «Видео» backgrounds (T4.4, FR-VIZ-005, `06` §7.4 — copy of Spotify Canvas: short silent
//! loops, `01a` §B.3). The person's own clips only: dropped on the window, **copied** (never
//! moved) into `<library>/.sirtone/backgrounds/`, tagged with a mood, served to `<video>` by
//! `http://sirtone-bg.localhost/<id>` with HTTP ranges (looping and seeking need 206 answers;
//! a range is read from the file, never the whole clip). No clips ship with the app until their
//! rights are settled (OWN-06). Checked here: extension and size (≤ 200 MB); the page checks
//! that the clip plays and lasts ≤ 60 s and removes it otherwise.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::ports::FileOps;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

use crate::library::Library;

pub const EXTS: &[&str] = &["webm", "mp4", "m4v"];
const MAX_BYTES: u64 = 200 * 1024 * 1024;
/// One answer carries at most this much of a clip.
const CHUNK: u64 = 4 * 1024 * 1024;
pub const MOODS: &[&str] = &["any", "happy", "energetic", "calm", "sad"];

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VideoClip {
    /// `<hex>.<ext>` — the file name in the backgrounds folder.
    pub id: String,
    /// `any | happy | energetic | calm | sad`.
    pub mood: String,
    /// The dropped file's name and size: the same file dropped twice is kept once.
    pub source: String,
    /// Size in bytes (f64 across IPC: no BigInt).
    pub bytes: f64,
}

/// Clips were added by a drop: their ids, and how many files were refused (type or size).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct VideosAdded {
    pub added: Vec<String>,
    pub refused: u32,
}

pub fn is_video(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

fn dir(app: &AppHandle) -> Option<PathBuf> {
    let root = app.try_state::<Library>()?.root()?;
    Some(
        root.join(sirtone_library::import::SERVICE_DIR)
            .join("backgrounds"),
    )
}

fn load(dir: &Path) -> Vec<VideoClip> {
    std::fs::read_to_string(dir.join("index.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(dir: &Path, clips: &[VideoClip]) -> std::io::Result<()> {
    let doc = serde_json::to_value(clips).map_err(std::io::Error::other)?;
    crate::settings_store::write_atomic(&dir.join("index.json"), &doc)
}

/// A valid clip id: hex, a dot, a known extension — nothing that walks out of the folder.
fn valid_id(id: &str) -> bool {
    let Some((name, ext)) = id.split_once('.') else {
        return false;
    };
    !name.is_empty()
        && name.len() <= 32
        && name.bytes().all(|b| b.is_ascii_hexdigit())
        && EXTS.contains(&ext)
}

/// Copies the dropped clips in (off the UI thread) and tells the page.
pub fn add_dropped(app: &AppHandle, paths: Vec<PathBuf>) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut added = Vec::new();
        let mut refused = 0u32;
        let Some(dir) = dir(&app) else {
            let _ = VideosAdded {
                added,
                refused: paths.len() as u32,
            }
            .emit(&app);
            return;
        };
        let _ = std::fs::create_dir_all(&dir);
        let mut clips = load(&dir);
        for p in paths {
            let Ok(meta) = std::fs::metadata(&p) else {
                refused += 1;
                continue;
            };
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_BYTES || !is_video(&p) {
                refused += 1;
                continue;
            }
            let source = format!(
                "{}:{}",
                p.file_name().and_then(|n| n.to_str()).unwrap_or(""),
                meta.len()
            );
            if let Some(c) = clips.iter().find(|c| c.source == source) {
                added.push(c.id.clone());
                continue;
            }
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos() as u64);
            let id = format!("{:016x}.{ext}", stamp ^ meta.len().rotate_left(32));
            let to = dir.join(&id);
            let tmp = dir.join(format!("{id}.part"));
            if std::fs::copy(&p, &tmp)
                .and_then(|_| std::fs::rename(&tmp, &to))
                .is_err()
            {
                let _ = std::fs::remove_file(&tmp);
                refused += 1;
                continue;
            }
            clips.push(VideoClip {
                id: id.clone(),
                mood: "any".into(),
                source,
                bytes: meta.len() as f64,
            });
            added.push(id);
        }
        let _ = save(&dir, &clips);
        let _ = VideosAdded { added, refused }.emit(&app);
    });
}

#[tauri::command]
#[specta::specta]
pub fn viz_videos(app: AppHandle) -> Vec<VideoClip> {
    dir(&app).map(|d| load(&d)).unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
pub fn viz_video_set_mood(app: AppHandle, id: String, mood: String) -> Result<(), AppError> {
    if !valid_id(&id) || !MOODS.contains(&mood.as_str()) {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    let dir = dir(&app).ok_or_else(|| AppError::new(ErrorCode::LibraryRootNotSet))?;
    let mut clips = load(&dir);
    let c = clips
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| AppError::new(ErrorCode::InvalidArgument))?;
    c.mood = mood;
    save(&dir, &clips).map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))
}

/// Removes a clip: our copy goes to the Recycle Bin (the dropped original was never touched).
#[tauri::command]
#[specta::specta]
pub fn viz_video_remove(app: AppHandle, id: String) -> Result<(), AppError> {
    if !valid_id(&id) {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    let dir = dir(&app).ok_or_else(|| AppError::new(ErrorCode::LibraryRootNotSet))?;
    let mut clips = load(&dir);
    clips.retain(|c| c.id != id);
    save(&dir, &clips).map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))?;
    let file = dir.join(&id);
    if file.exists() {
        sirtone_platform::files::WinFiles
            .to_recycle_bin(&file)
            .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))?;
    }
    Ok(())
}

/// The requested byte range (`bytes=a-b`, `bytes=a-`), clipped to `CHUNK` and the file.
fn range(header: Option<&str>, len: u64) -> Option<(u64, u64)> {
    if len == 0 {
        return None;
    }
    let (start, end) = match header.and_then(|h| h.strip_prefix("bytes=")) {
        Some(spec) => {
            let (a, b) = spec.split_once('-')?;
            if a.is_empty() {
                // `bytes=-n`: the last n bytes.
                let n: u64 = b.parse().ok()?;
                (len.saturating_sub(n), len - 1)
            } else {
                let a: u64 = a.parse().ok()?;
                let b = if b.is_empty() {
                    len - 1
                } else {
                    b.parse::<u64>().ok()?.min(len - 1)
                };
                (a, b)
            }
        }
        None => (0, len - 1),
    };
    if start > end || start >= len {
        return None;
    }
    Some((start, end.min(start + CHUNK - 1)))
}

/// `http://sirtone-bg.localhost/<id>`: only clips listed in the folder's index answer.
pub fn protocol(
    app: &AppHandle,
    path: &str,
    range_header: Option<&str>,
) -> tauri::http::Response<Vec<u8>> {
    let not_found = || {
        tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
    };
    let id = path.trim_matches('/');
    let Some(dir) = dir(app) else {
        return not_found();
    };
    if !valid_id(id) || !load(&dir).iter().any(|c| c.id == id) {
        return not_found();
    }
    let Ok(mut f) = std::fs::File::open(dir.join(id)) else {
        return not_found();
    };
    let len = f.metadata().map_or(0, |m| m.len());
    let Some((start, end)) = range(range_header, len) else {
        return tauri::http::Response::builder()
            .status(416)
            .header("Content-Range", format!("bytes */{len}"))
            .body(Vec::new())
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()));
    };
    let mut buf = vec![0u8; (end - start + 1) as usize];
    if f.seek(SeekFrom::Start(start)).is_err() || f.read_exact(&mut buf).is_err() {
        return not_found();
    }
    let mime = if id.ends_with(".webm") {
        "video/webm"
    } else {
        "video/mp4"
    };
    tauri::http::Response::builder()
        .status(206)
        .header("Content-Type", mime)
        .header("Accept-Ranges", "bytes")
        .header("Content-Range", format!("bytes {start}-{end}/{len}"))
        .header("Content-Length", buf.len().to_string())
        .body(buf)
        .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_clipped_and_checked() {
        let mb = 1024 * 1024;
        assert_eq!(range(None, 10), Some((0, 9)));
        assert_eq!(range(Some("bytes=0-"), 100 * mb), Some((0, CHUNK - 1)));
        assert_eq!(range(Some("bytes=5-9"), 100), Some((5, 9)));
        assert_eq!(range(Some("bytes=90-200"), 100), Some((90, 99)));
        assert_eq!(range(Some("bytes=-10"), 100), Some((90, 99)));
        assert_eq!(range(Some("bytes=100-"), 100), None);
        assert_eq!(range(Some("bytes=x-"), 100), None);
        assert_eq!(range(Some("bytes=0-"), 0), None);
    }

    #[test]
    fn ids_cannot_leave_the_folder() {
        assert!(valid_id("00ff00ff00ff00ff.webm"));
        assert!(!valid_id("../index.json"));
        assert!(!valid_id("abc.exe"));
        assert!(!valid_id("abc.webm/../../x"));
        assert!(!valid_id(".webm"));
        assert!(is_video(Path::new("C:/x/Clip.MP4")));
        assert!(!is_video(Path::new("C:/x/song.flac")));
    }
}
