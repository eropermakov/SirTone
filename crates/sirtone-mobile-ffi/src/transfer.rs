//! Transfer on the phone (M6, D-140): the hub drops a package into `Documents` over the cable
//! (`house_arrest`/`afc`); the app applies it on open. «Забрать с телефона» — the app writes its
//! own package for the hub to take. Same format and rules as the PC (`sirtone-sync`).

use std::path::{Path, PathBuf};

use sirtone_transfer::{Parts, Platform};

use crate::CoreError;
use crate::library::Library;

fn code(e: &sirtone_sync::SyncError) -> CoreError {
    CoreError::Failed {
        code: e.code().into(),
    }
}

/// «Перенесено 1 240 треков» (MR §5) and what came with them.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TransferResult {
    pub tracks: u32,
    pub new_tracks: u32,
    pub plays: u32,
    pub marks: u32,
    pub playlists: u32,
}

#[uniffi::export]
impl Library {
    /// Applies the package in `dir`: new music through the phone import, then plays, marks and
    /// playlists, each once (applying the same package again changes nothing).
    pub fn apply_package(&self, dir: String) -> Result<TransferResult, CoreError> {
        let root = PathBuf::from(self.root());
        self.with(|conn| {
            sirtone_sync::apply(conn, Path::new(&dir), &mut |db, files| {
                crate::import::import_with(db, &root, files)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
        })
        .map(|s| TransferResult {
            tracks: s.tracks,
            new_tracks: s.new_files,
            plays: s.plays,
            marks: s.marks,
            playlists: s.playlists,
        })
        .map_err(|e| code(&e))
    }

    /// Writes this library's package into `dir` for the hub («Забрать с телефона»). `music` —
    /// with the audio files; resumes when run again.
    pub fn export_package(&self, dir: String, music: bool) -> Result<u32, CoreError> {
        let root = PathBuf::from(self.root());
        let parts = Parts {
            music,
            playlists: true,
            taste: true,
            settings: false,
        };
        let platform = if cfg!(target_os = "android") {
            Platform::Android
        } else {
            Platform::Ios
        };
        self.with(|conn| {
            sirtone_sync::export(
                conn,
                &root,
                Path::new(&dir),
                platform,
                env!("CARGO_PKG_VERSION"),
                parts,
            )
        })
        .map(|s| s.tracks)
        .map_err(|e| code(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Phone A writes a package with its music; phone B applies it twice: the music arrives once
    /// through the import, the ♡ comes along, nothing doubles.
    #[test]
    fn a_package_travels_between_two_libraries() {
        let base = std::env::temp_dir().join(format!(
            "sirtone-phone-transfer-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let inbox = base.join("in");
        std::fs::create_dir_all(&inbox).unwrap();
        crate::player::tests::wav(&inbox.join("Песня.wav"));

        let a = Library::open(base.join("a").to_string_lossy().into_owned()).unwrap();
        a.import_files(vec![inbox.to_string_lossy().into_owned()])
            .unwrap();
        let id = a.tracks(None, 10).unwrap().tracks[0].id;
        a.set_like(id, true).unwrap();
        let pkg = base.join("pkg").to_string_lossy().into_owned();
        assert_eq!(a.export_package(pkg.clone(), true).unwrap(), 1);

        let b = Library::open(base.join("b").to_string_lossy().into_owned()).unwrap();
        let r = b.apply_package(pkg.clone()).unwrap();
        assert_eq!((r.tracks, r.new_tracks, r.marks), (1, 1, 1), "{r:?}");
        assert_eq!(b.counts().unwrap().tracks, 1);
        let r2 = b.apply_package(pkg).unwrap();
        assert_eq!((r2.new_tracks, r2.plays, r2.marks), (0, 0, 0), "{r2:?}");
        assert_eq!(b.counts().unwrap().tracks, 1);
        drop((a, b));
        let _ = std::fs::remove_dir_all(&base);
    }
}
