//! Transfer package between the PC and a phone (D-134, D-140, `docs/mobile/README.md` §11).
//!
//! A package is a folder: `manifest.json` (what is inside), the music files under `music/`, and
//! `events.jsonl` (the listening journal and marks). The format is the same in both directions
//! and on every platform. This crate only describes the package and decides how it merges into
//! a library; the apps copy files (on the iPhone through `house_arrest`/`afc`) and write the
//! library through their own repositories. Audio is never re-encoded.
//!
//! Merge rules (D-140): tracks are matched by sound (fingerprint), else by artist + title +
//! duration; events carry a global key and are applied once; for ♡ the latest mark wins; the
//! wave model is rebuilt from the merged journal (weights are never added up).

pub mod events;
pub mod manifest;
pub mod merge;

pub use events::{Event, EventKind};
pub use manifest::{Manifest, Parts, Platform, Source, TrackEntry};

/// Package format version written by this build. Readers accept this and older versions.
pub const FORMAT: u32 = 1;
/// File names inside a package folder.
pub const MANIFEST_FILE: &str = "manifest.json";
pub const EVENTS_FILE: &str = "events.jsonl";
pub const MUSIC_DIR: &str = "music";

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Made by a newer Сиртон: the user updates the app that reads it.
    #[error("package format {0} is newer than {FORMAT}")]
    NewerFormat(u32),
    /// A path inside the package leaves the package folder or is not portable.
    #[error("unsafe path in package: {0}")]
    UnsafePath(String),
}

impl TransferError {
    /// Stable code for the IPC/FFI boundary; the UI builds the text (project rule).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "transfer.io",
            Self::Json(_) => "transfer.corrupt",
            Self::NewerFormat(_) => "transfer.newer_format",
            Self::UnsafePath(_) => "transfer.unsafe_path",
        }
    }
}

/// A package-relative path is safe: `/`-separated, not empty, no absolute or drive prefix, no
/// `.`/`..` parts, no backslash, no NUL, no Windows-reserved characters.
pub fn safe_rel_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains(['\\', '\0', ':', '*', '?', '"', '<', '>', '|'])
        && p.split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.ends_with(' '))
}

#[cfg(test)]
mod tests {
    use super::safe_rel_path;

    #[test]
    fn paths() {
        assert!(safe_rel_path("music/Артист/Альбом/01 Трек.flac"));
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "music/../../x",
            "music\\x",
            "C:/x",
            "a//b",
            "a/./b",
            "a/b ",
            "a\0b",
        ] {
            assert!(!safe_rel_path(bad), "{bad:?}");
        }
    }
}
