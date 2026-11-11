//! Managing the phone's music (D-165, owner's list 2026-11-11 p. 5): out of a playlist, a
//! playlist renamed or deleted, tracks deleted from the phone. A deleted track's files go to the
//! library's own trash (never destroyed by the app), its row goes, and it is remembered in
//! `deleted.json` so the hub never sends it back (Q-41). The PC is never touched.

use std::path::Path;

use rusqlite::OptionalExtension;
use sirtone_core::ports::FileOps as _;
use sirtone_library::{playlists, relocate};

use crate::CoreError;
use crate::import::PhoneFiles;
use crate::library::Library;

const SERVICE_DIR: &str = ".sirtone";

/// What a deletion would free or freed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Deletion {
    pub tracks: u32,
    pub bytes: u64,
}

fn failed(code: &str) -> CoreError {
    CoreError::Failed { code: code.into() }
}

#[uniffi::export]
impl Library {
    /// Takes every occurrence of a track out of a manual playlist (the track stays).
    pub fn remove_from_playlist(&self, playlist_id: i64, track_id: i64) -> Result<(), CoreError> {
        self.with(|c| {
            let at: Vec<i64> = playlists::tracks(c, playlist_id)?
                .into_iter()
                .filter(|t| t.track.id == track_id)
                .map(|t| t.position)
                .collect();
            playlists::remove(c, playlist_id, &at)
        })?;
        Ok(())
    }

    pub fn rename_playlist(&self, playlist_id: i64, name: String) -> Result<(), CoreError> {
        Ok(self.with(|c| playlists::rename(c, playlist_id, &name))?)
    }

    /// The playlist goes; its tracks stay.
    pub fn delete_playlist(&self, playlist_id: i64) -> Result<(), CoreError> {
        Ok(self.with(|c| playlists::delete(c, playlist_id))?)
    }

    /// What deleting these tracks would free: originals and copies on the phone.
    pub fn delete_size(&self, track_ids: Vec<i64>) -> Result<Deletion, CoreError> {
        self.with(|c| {
            let mut f = Deletion {
                tracks: 0,
                bytes: 0,
            };
            let mut st = c.prepare_cached(
                "SELECT size_bytes, coalesce(copy_bytes, 0) FROM track WHERE id = ?1",
            )?;
            for id in &track_ids {
                if let Some((a, b)) = st
                    .query_row([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
                    .optional()?
                {
                    f.tracks += 1;
                    f.bytes += u64::try_from(a + b).unwrap_or(0);
                }
            }
            Ok::<_, sirtone_library::LibraryError>(f)
        })
        .map_err(Into::into)
    }

    /// «Удалить с телефона»: the files to the library's trash, the rows out, the tracks
    /// remembered as deleted. Returns what was deleted.
    pub fn delete_tracks(&self, track_ids: Vec<i64>) -> Result<Deletion, CoreError> {
        let root = std::path::PathBuf::from(self.root());
        let freed = self.delete_size(track_ids.clone())?;
        let files = PhoneFiles { root: root.clone() };
        let mut gone = Vec::new();
        for id in &track_ids {
            let who = self.with(|c| {
                c.query_row(
                    "SELECT t.title, t.duration_ms, t.content_hash, t.copy_rel_path,
                            (SELECT a.name FROM track_artist ta JOIN artist a ON a.id = ta.artist_id
                             WHERE ta.track_id = t.id ORDER BY ta.position LIMIT 1)
                       FROM track t WHERE t.id = ?1",
                    [id],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, Option<String>>(3)?,
                            r.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .optional()
                .map_err(sirtone_library::LibraryError::from)
            })?;
            let Some((title, duration, hash, copy, artist)) = who else {
                continue;
            };
            // The economy copy beside the original (D-146) goes to the trash too.
            if let Some(rel) = copy {
                let p = relocate_path(&root, &rel);
                if p.exists() {
                    files
                        .to_recycle_bin(&p)
                        .map_err(|_| failed("manage.trash"))?;
                }
            }
            self.with(|c| relocate::remove(c, &root, *id, true, &files))?;
            gone.push(sirtone_sync::Gone {
                artist,
                title,
                duration_ms: u32::try_from(duration).unwrap_or(0),
                hash,
            });
        }
        let list = root.join(SERVICE_DIR).join(sirtone_sync::GONE_FILE);
        sirtone_sync::add_gone(&list, &gone).map_err(|_| failed("manage.gone_list"))?;
        Ok(freed)
    }

    /// The size of the library's trash (deleted music waiting there).
    pub fn trash_bytes(&self) -> u64 {
        let dir = Path::new(&self.root()).join(SERVICE_DIR).join("trash");
        std::fs::read_dir(dir)
            .map(|it| {
                it.filter_map(Result::ok)
                    .filter_map(|e| e.metadata().ok())
                    .filter(std::fs::Metadata::is_file)
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0)
    }

    /// «Очистить корзину» in «Хранилище»: the person's own word frees the space for good.
    pub fn empty_trash(&self) -> Result<u64, CoreError> {
        let freed = self.trash_bytes();
        let dir = Path::new(&self.root()).join(SERVICE_DIR).join("trash");
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(freed),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(_) => Err(failed("manage.empty_trash")),
        }
    }
}

fn relocate_path(root: &Path, rel: &str) -> std::path::PathBuf {
    rel.split('/')
        .fold(root.to_path_buf(), |p, part| p.join(part))
}

#[cfg(test)]
mod tests {
    use crate::library::tests::sample;

    #[test]
    fn playlists_and_deletion() {
        let (lib, dir) = sample();
        let t = lib.tracks(None, 10).unwrap().tracks;
        let pl = lib.create_playlist("В дорогу".into()).unwrap();
        lib.add_to_playlist(pl, vec![t[0].id, t[1].id, t[0].id])
            .unwrap();
        lib.remove_from_playlist(pl, t[0].id).unwrap();
        let left: Vec<i64> = lib
            .playlist_tracks(pl)
            .unwrap()
            .iter()
            .map(|x| x.id)
            .collect();
        assert_eq!(left, [t[1].id], "every occurrence goes, the others stay");
        lib.rename_playlist(pl, "Домой".into()).unwrap();
        assert_eq!(lib.playlists().unwrap()[0].name, "Домой");

        let size = lib.delete_size(vec![t[1].id]).unwrap();
        assert_eq!(size.tracks, 1);
        let path = lib.track_path(t[1].id).unwrap();
        // The sample has rows only: the file to delete.
        std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
        std::fs::write(&path, b"flac").unwrap();
        let freed = lib.delete_tracks(vec![t[1].id]).unwrap();
        assert_eq!(freed, size);
        assert!(
            !std::path::Path::new(&path).exists(),
            "the file left the library"
        );
        assert!(lib.trash_bytes() > 0, "into the trash, not destroyed");
        assert_eq!(lib.tracks(None, 10).unwrap().tracks.len(), t.len() - 1);
        assert!(lib.playlist_tracks(pl).unwrap().is_empty());
        let gone = sirtone_sync::read_gone(&dir.join(".sirtone").join(sirtone_sync::GONE_FILE));
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].title, t[1].title);
        assert!(lib.empty_trash().unwrap() > 0);
        assert_eq!(lib.trash_bytes(), 0);

        lib.delete_playlist(pl).unwrap();
        assert!(lib.playlists().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
