//! «Добавить музыку» on the phone (M4, FR-MOB-008): the PC import pipeline (copy, verify,
//! recognise, lay out, duplicates by content, «Неизвестные») with phone ports instead of the
//! Windows ones. The source files (the system picker, «Поделиться», «Входящие») are only read.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use sirtone_core::ports::{FileOps, VolumeInfo};
use sirtone_library::import::{self, ImportDeps, SERVICE_DIR};

use crate::CoreError;
use crate::library::Library;

/// File operations on the phone's own storage (one volume, the app's sandbox).
struct PhoneFiles {
    root: PathBuf,
}

impl FileOps for PhoneFiles {
    /// Never overwrites: a hard link fails when `to` exists, then the old name goes away. The
    /// directory entry is flushed so the move survives a crash.
    fn move_durable(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::hard_link(from, to).or_else(|e| {
            // Some file systems have no hard links: check, then rename (the sandbox has no
            // other writer).
            if to.exists() {
                Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, e))
            } else {
                std::fs::rename(from, to)
            }
        })?;
        if from.exists() {
            std::fs::remove_file(from)?;
        }
        if let Some(dir) = to.parent()
            && let Ok(d) = std::fs::File::open(dir)
        {
            let _ = d.sync_all();
        }
        Ok(())
    }

    /// No Recycle Bin on a phone: «deleted» files go to the library's own trash folder — data is
    /// never destroyed by the app (project rule); the user empties it from «Хранилище».
    fn to_recycle_bin(&self, path: &Path) -> std::io::Result<()> {
        let trash = self.root.join(SERVICE_DIR).join("trash");
        std::fs::create_dir_all(&trash)?;
        let name = path.file_name().unwrap_or_default();
        let mut to = trash.join(name);
        let mut n = 1;
        while to.exists() {
            to = trash.join(format!("{n} {}", name.to_string_lossy()));
            n += 1;
        }
        std::fs::rename(path, to)
    }
}

/// The app's storage is internal; free space is checked by the system picker and the app.
struct PhoneVolume;

impl VolumeInfo for PhoneVolume {
    fn is_removable(&self, _path: &Path) -> bool {
        false
    }

    fn free_bytes(&self, _path: &Path) -> Option<u64> {
        None
    }
}

/// «Добавлено 38, в Неизвестных 2» (MR §5) and what was skipped.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ImportSummary {
    pub added: u64,
    pub unknown: u64,
    pub duplicates: u64,
    /// Not music, broken or unsupported — skipped quietly (MR §5), counted for the log.
    pub skipped: u64,
}

#[uniffi::export]
impl Library {
    /// Imports files or folders into the library. Each track can be played as soon as it is in
    /// (the app refreshes lists on return); analysis comes later in the background.
    pub fn import_files(&self, paths: Vec<String>) -> Result<ImportSummary, CoreError> {
        let root = PathBuf::from(self.root());
        let sources: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        self.with(|conn| import_with(conn, &root, &sources))
            .map_err(Into::into)
    }
}

/// The phone import on an open library (also the transfer's «new music» step).
pub(crate) fn import_with(
    conn: &mut rusqlite::Connection,
    root: &Path,
    sources: &[PathBuf],
) -> Result<ImportSummary, sirtone_library::LibraryError> {
    {
        let root = root.to_path_buf();
        let deps = ImportDeps {
            probe: Arc::new(sirtone_audio::probe::SymphoniaProbe),
            tags: Arc::new(sirtone_recognize::LoftyTags),
            identify: Arc::new(sirtone_recognize::TagIdentify),
            fs: Arc::new(PhoneFiles { root: root.clone() }),
            volume: Arc::new(PhoneVolume),
            covers: Arc::new(sirtone_recognize::LoftyCovers),
        };
        let cancel = AtomicBool::new(false);
        {
            // A batch cut by the system (app killed) finishes first.
            import::recover(conn, &root, deps.fs.as_ref())?;
            let mut batches = import::unfinished_batches(conn)?;
            batches.push(import::start_batch(conn, sources, "external")?);
            let mut sum = ImportSummary {
                added: 0,
                unknown: 0,
                duplicates: 0,
                skipped: 0,
            };
            for b in batches {
                let r = import::run_batch(conn, &root, b, &deps, &cancel, &mut |_| {})?;
                sum.added += r.added;
                sum.unknown += r.unknown;
                sum.duplicates += r.duplicates.len() as u64;
                sum.skipped += r.rejected.len() as u64;
            }
            Ok(sum)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_a_file_and_skips_rubbish_and_duplicates() {
        let base = std::env::temp_dir().join(format!(
            "sirtone-phone-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let inbox = base.join("Входящие");
        std::fs::create_dir_all(&inbox).unwrap();
        let song = inbox.join("Неизвестный трек.wav");
        crate::player::tests::wav(&song);
        std::fs::write(inbox.join("заметка.txt"), "не музыка").unwrap();

        let lib = Library::open(base.join("library").to_string_lossy().into_owned()).unwrap();
        let s = lib
            .import_files(vec![inbox.to_string_lossy().into_owned()])
            .unwrap();
        assert_eq!(s.added, 1, "{s:?}");
        assert_eq!(s.unknown, 1, "no tags → «Неизвестные», not a wrong folder");
        assert!(song.exists(), "the source is only read");
        let n = lib.counts().unwrap();
        assert_eq!((n.tracks, n.unknown), (1, 1));

        // The same file again: a duplicate, nothing added.
        let again = lib
            .import_files(vec![song.to_string_lossy().into_owned()])
            .unwrap();
        assert_eq!((again.added, again.duplicates), (0, 1), "{again:?}");
        drop(lib);
        let _ = std::fs::remove_dir_all(&base);
    }
}
