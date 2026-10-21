//! Portable mode (T6.4, FR-PORT-001/002, `03` §8.2, D-115): a `sirtone.portable` file next to the
//! exe. Then everything lives in that folder — settings, logs, the WebView profile in `Data`, the
//! library in `Music` (stored relative to the folder: the stick gets another drive letter on
//! another PC); nothing is written to the host — no autostart, no updater (a self-replacing exe on
//! a stick), no Jump List, no file associations; the database leaves WAL on a removable disk by
//! itself (`sirtone-library::db`). A WebView2 Fixed Runtime in `WebView2Runtime` next to the exe is
//! used when present (PCs without WebView2). Its own single-instance identity, so a portable copy
//! never just focuses an installed Сиртон.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const MARKER: &str = "sirtone.portable";
pub const DATA: &str = "Data";
pub const MUSIC: &str = "Music";
pub const RUNTIME: &str = "WebView2Runtime";

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// The portable folder (the exe's), when the marker is there.
pub fn root() -> Option<&'static Path> {
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();
    ROOT.get_or_init(|| exe_dir().filter(|d| d.join(MARKER).is_file()))
        .as_deref()
}

pub fn is_portable() -> bool {
    root().is_some()
}

/// Settings, logs, the WebView profile.
pub fn data_dir() -> Option<PathBuf> {
    root().map(|r| r.join(DATA))
}

/// The library folder's default: `Music` next to the exe.
pub fn default_library() -> Option<PathBuf> {
    root().map(|r| r.join(MUSIC))
}

/// A stored library path → the real one: relative ones hang off the portable folder.
pub fn resolve(stored: &str) -> PathBuf {
    let p = PathBuf::from(stored);
    match root() {
        Some(r) if p.is_relative() => r.join(p),
        _ => p,
    }
}

/// The real library path → what is stored: inside the portable folder — relative.
pub fn store(path: &Path) -> String {
    match root().and_then(|r| path.strip_prefix(r).ok()) {
        Some(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().into_owned(),
        _ => path.to_string_lossy().into_owned(),
    }
}

/// Before anything starts (first thing in `main`): the WebView profile in `Data`, the Fixed
/// Runtime when it is there.
pub fn prepare() {
    let Some(r) = root() else { return };
    // SAFETY: called first in `main`, before any other thread exists.
    unsafe {
        std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", r.join(DATA).join("webview"));
        let rt = r.join(RUNTIME);
        if rt.join("msedgewebview2.exe").is_file() {
            std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", rt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_runner_is_not_portable_and_paths_pass_through() {
        // No marker next to the test binary: installed behaviour.
        assert!(!is_portable());
        assert_eq!(
            resolve("D:/Music/SirTone"),
            PathBuf::from("D:/Music/SirTone")
        );
        assert_eq!(store(Path::new("D:/Music/SirTone")), "D:/Music/SirTone");
    }
}
