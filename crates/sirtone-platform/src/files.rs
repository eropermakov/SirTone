//! Windows file operations for the library (docs/03 §8.3): durable moves, Recycle Bin, volume facts.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use sirtone_core::ports::{FileOps, VolumeInfo};
use windows::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetVolumePathNameW, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use windows::core::PCWSTR;

/// Absolute path with the `\\?\` prefix, so moves work past MAX_PATH regardless of the manifest.
fn long(p: &Path) -> Vec<u16> {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let s = abs.as_os_str().to_string_lossy();
    let prefixed: PathBuf = if s.starts_with(r"\\?\") {
        abs
    } else if let Some(unc) = s.strip_prefix(r"\\") {
        PathBuf::from(format!(r"\\?\UNC\{unc}"))
    } else {
        PathBuf::from(format!(r"\\?\{s}"))
    };
    wide(prefixed.as_os_str())
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

pub struct WinFiles;

impl FileOps for WinFiles {
    fn move_durable(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        let (f, t) = (long(from), long(to));
        // No MOVEFILE_REPLACE_EXISTING: an existing target is an error, never overwritten.
        // SAFETY: both buffers are NUL-terminated and live across the call.
        unsafe {
            MoveFileExW(
                PCWSTR(f.as_ptr()),
                PCWSTR(t.as_ptr()),
                MOVEFILE_WRITE_THROUGH,
            )
        }
        .map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF))
    }

    fn to_recycle_bin(&self, path: &Path) -> std::io::Result<()> {
        trash::delete(path).map_err(std::io::Error::other)
    }
}

/// Volume root of `path` (`D:\`, a mount point or `\\server\share\`).
fn volume_root(path: &Path) -> Option<Vec<u16>> {
    let p = wide(std::path::absolute(path).ok()?.as_os_str());
    let mut buf = [0u16; 512];
    // SAFETY: `p` is NUL-terminated, `buf` is a valid writable buffer.
    unsafe { GetVolumePathNameW(PCWSTR(p.as_ptr()), &mut buf) }.ok()?;
    let len = buf.iter().position(|&c| c == 0)?;
    Some(buf[..=len].to_vec())
}

/// `GetDriveTypeW` result for removable media (winbase.h `DRIVE_REMOVABLE`).
const DRIVE_REMOVABLE: u32 = 2;

pub struct WinVolume;

impl VolumeInfo for WinVolume {
    fn is_removable(&self, path: &Path) -> bool {
        volume_root(path).is_some_and(|root| {
            // SAFETY: NUL-terminated root path.
            let kind = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
            kind == DRIVE_REMOVABLE
        })
    }

    fn free_bytes(&self, path: &Path) -> Option<u64> {
        let root = volume_root(path)?;
        let mut free = 0u64;
        // SAFETY: NUL-terminated root, valid out-pointer.
        unsafe { GetDiskFreeSpaceExW(PCWSTR(root.as_ptr()), Some(&mut free), None, None) }.ok()?;
        Some(free)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn durable_move_never_overwrites_and_handles_long_unicode_paths() {
        let dir = std::env::temp_dir().join(format!("sirtone-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let deep = dir
            .join("Очень длинное имя папки ".repeat(6).trim())
            .join("ещё глубже 🎵".repeat(4));
        fs::create_dir_all(&deep).unwrap_or_else(|_| fs::create_dir_all(&dir).unwrap());
        let a = dir.join("a.txt");
        let b = if deep.exists() {
            deep.join("b.txt")
        } else {
            dir.join("b.txt")
        };
        fs::write(&a, "1").unwrap();
        WinFiles.move_durable(&a, &b).unwrap();
        assert!(!a.exists() && b.exists());
        fs::write(&a, "2").unwrap();
        assert!(WinFiles.move_durable(&a, &b).is_err(), "must not overwrite");
        assert_eq!(fs::read_to_string(&b).unwrap(), "1");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volume_facts_for_temp_dir() {
        let tmp = std::env::temp_dir();
        assert!(WinVolume.free_bytes(&tmp).is_some_and(|f| f > 0));
        assert!(!WinVolume.is_removable(&tmp));
    }
}
