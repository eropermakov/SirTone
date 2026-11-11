//! The phone's library (M4, FR-MOB-001): the same `library.db` schema and migrations as the PC,
//! kept in the app's own storage (`Application Support`, excluded from iCloud backup — the app
//! passes the folder). Every call is short and synchronous; Swift/Kotlin call it off the main
//! thread. Lists come in pages (FR-MOB-010: 10 000 tracks scroll without waiting).

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;
use sirtone_library::{LibraryError, Medium, collection, feedback, repo, search};

use crate::CoreError;

/// Folder inside the library root that holds the database, covers and journals (as on the PC).
const SERVICE_DIR: &str = ".sirtone";

impl From<LibraryError> for CoreError {
    fn from(e: LibraryError) -> Self {
        // Stable codes only; the details stay in the core's log, never in the UI.
        let code = match e {
            LibraryError::Invalid(_) => "library.invalid",
            _ => "library.db",
        };
        Self::Failed { code: code.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LibraryCounts {
    pub tracks: u64,
    pub artists: u64,
    pub albums: u64,
    pub unknown: u64,
    pub duration_ms: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TrackItem {
    pub id: i64,
    pub title: String,
    pub artists: String,
    pub album: Option<String>,
    pub duration_ms: u64,
    /// For «К альбому» / «К исполнителю» in the track menu.
    pub album_id: Option<i64>,
    pub artist_id: Option<i64>,
}

/// Keyset cursor of the «Треки» list (by title); opaque to the app.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TrackCursor {
    pub title_norm: String,
    pub id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TracksPage {
    pub tracks: Vec<TrackItem>,
    pub next: Option<TrackCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NamedItem {
    pub id: i64,
    pub name: String,
}

/// Search groups (S03): results from the first letter, typos, layout, transliteration, ё/е.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SearchResults {
    pub tracks: Vec<TrackItem>,
    pub artists: Vec<NamedItem>,
    pub albums: Vec<NamedItem>,
}

pub(crate) fn item(t: repo::TrackRow) -> TrackItem {
    TrackItem {
        id: t.id,
        title: t.title,
        artists: t.artists,
        album: t.album,
        duration_ms: t.duration_ms,
        album_id: t.album_id,
        artist_id: t.artist_id,
    }
}

#[derive(uniffi::Object)]
pub struct Library {
    root: PathBuf,
    conn: Mutex<Connection>,
    /// Long work in progress (import, a package from the PC): read without the connection's
    /// lock, so the app can show it while the core is busy.
    pub(crate) work: Work,
}

/// Files done of files to do in the running import; 0 of 0 when idle.
#[derive(Debug, Default)]
pub(crate) struct Work {
    done: std::sync::atomic::AtomicU32,
    total: std::sync::atomic::AtomicU32,
}

impl Work {
    pub(crate) fn set(&self, done: u64, total: u64) {
        use std::sync::atomic::Ordering::Relaxed;
        self.total
            .store(u32::try_from(total).unwrap_or(u32::MAX), Relaxed);
        self.done
            .store(u32::try_from(done).unwrap_or(u32::MAX), Relaxed);
    }
}

/// What the app shows under «Разбираю…»: `done` of `total` files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct WorkProgress {
    pub done: u32,
    pub total: u32,
}

impl Library {
    fn db(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves a consistent SQLite connection: keep going.
        self.conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The connection for other modules of this crate (transfer, wave — next steps of M4–M6).
    pub(crate) fn with<T>(&self, f: impl FnOnce(&mut Connection) -> T) -> T {
        f(&mut self.db())
    }
}

#[uniffi::export]
impl Library {
    /// Opens the library in `root`, creating it the first time (migrations go forward only).
    #[uniffi::constructor]
    pub fn open(root: String) -> Result<std::sync::Arc<Self>, CoreError> {
        let root = PathBuf::from(root);
        let conn =
            sirtone_library::open(&root.join(SERVICE_DIR).join("library.db"), Medium::Fixed)?;
        Ok(std::sync::Arc::new(Self {
            root,
            conn: Mutex::new(conn),
            work: Work::default(),
        }))
    }

    /// The running import's progress (polled by the app; never waits for the core).
    pub fn work_progress(&self) -> WorkProgress {
        use std::sync::atomic::Ordering::Relaxed;
        WorkProgress {
            done: self.work.done.load(Relaxed),
            total: self.work.total.load(Relaxed),
        }
    }

    pub fn root(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    pub fn counts(&self) -> Result<LibraryCounts, CoreError> {
        let c = collection::counts(&self.db())?;
        Ok(LibraryCounts {
            tracks: c.tracks,
            artists: c.artists,
            albums: c.albums,
            unknown: c.unknown,
            duration_ms: c.duration_ms,
            bytes: c.bytes,
        })
    }

    /// One page of every track by title (S02 «Треки»).
    pub fn tracks(&self, after: Option<TrackCursor>, limit: u32) -> Result<TracksPage, CoreError> {
        let cursor = after.map(|c| repo::TitleCursor {
            title_norm: c.title_norm,
            id: c.id,
        });
        let (rows, next) = repo::tracks_by_title(&self.db(), cursor.as_ref(), limit.min(500))?;
        Ok(TracksPage {
            tracks: rows.into_iter().map(item).collect(),
            next: next.map(|c| TrackCursor {
                title_norm: c.title_norm,
                id: c.id,
            }),
        })
    }

    pub fn search(&self, query: String) -> Result<SearchResults, CoreError> {
        let mut r = search::search(&self.db(), &query)?;
        // «Лучшее совпадение» comes apart on the PC; the phone shows it first in its group.
        match r.best.take() {
            Some(search::Best::Track(b)) if !r.tracks.iter().any(|t| t.id == b.id) => {
                r.tracks.insert(0, b);
            }
            Some(search::Best::Artist(b)) if !r.artists.iter().any(|a| a.id == b.id) => {
                r.artists.insert(0, b);
            }
            Some(search::Best::Album(b)) if !r.albums.iter().any(|a| a.id == b.id) => {
                r.albums.insert(0, b);
            }
            _ => {}
        }
        Ok(SearchResults {
            tracks: r.tracks.into_iter().map(item).collect(),
            artists: r
                .artists
                .into_iter()
                .map(|a| NamedItem {
                    id: a.id,
                    name: a.name,
                })
                .collect(),
            albums: r
                .albums
                .into_iter()
                .map(|a| NamedItem {
                    id: a.id,
                    name: a.title,
                })
                .collect(),
        })
    }

    /// ♡ (optimistic in the UI; on error the app rolls back quietly and shows a toast).
    pub fn set_like(&self, track_id: i64, on: bool) -> Result<(), CoreError> {
        feedback::set_like(&self.db(), track_id, on)?;
        Ok(())
    }

    /// «Никогда в волне» (PC `settingsNever`): the wave never picks the track; off again from
    /// «Настройки → Волна».
    pub fn set_never(&self, track_id: i64, on: bool) -> Result<(), CoreError> {
        feedback::set_never_track(&mut self.db(), track_id, on)?;
        Ok(())
    }

    /// The tracks under «Никогда в волне», by title.
    pub fn never_tracks(&self) -> Result<Vec<TrackItem>, CoreError> {
        let ids: Vec<i64> = self.with(|c| -> Result<Vec<i64>, sirtone_library::LibraryError> {
            let mut st =
                c.prepare("SELECT id FROM track WHERE wave_never = 1 ORDER BY title_norm, id")?;
            let ids = st
                .query_map([], |r| r.get(0))?
                .collect::<Result<Vec<i64>, _>>()?;
            Ok(ids)
        })?;
        self.tracks_by_ids(ids)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use sirtone_library::repo::{
        Identity, NewTrack, album_get_or_create, artist_get_or_create, track_insert,
    };

    /// A library in a fresh temp folder with a few tracks.
    pub(crate) fn sample() -> (std::sync::Arc<Library>, PathBuf) {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sirtone-mobile-{}-{}-{}",
            std::process::id(),
            // Parallel tests can share a clock tick on Windows: a counter keeps them apart.
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let lib = Library::open(dir.to_string_lossy().into_owned()).unwrap();
        lib.with(|c| {
            let kino = artist_get_or_create(c, "Кино").unwrap();
            let album = album_get_or_create(c, "Группа крови", kino, Some(1988)).unwrap();
            for (no, title) in [
                (1, "Группа крови"),
                (2, "Закрой за мной дверь"),
                (3, "Ёлка"),
            ] {
                track_insert(
                    c,
                    &NewTrack {
                        rel_path: format!("Кино/Группа крови/{no:02} {title}.flac"),
                        size_bytes: 1_000,
                        mtime_ms: 0,
                        content_hash: format!("h{no}"),
                        container: "flac".into(),
                        codec: "flac".into(),
                        lossless: true,
                        sample_rate: Some(44100),
                        channels: Some(2),
                        duration_ms: 200_000,
                        title: title.into(),
                        album_id: Some(album),
                        disc_no: None,
                        track_no: Some(no),
                        year: Some(1988),
                        identity: Identity::Inferred,
                        identity_conf: 0.9,
                        unknown_reason: None,
                        artists: vec![kino],
                    },
                )
                .unwrap();
            }
        });
        (lib, dir)
    }

    #[test]
    fn counts_pages_search_like() {
        let (lib, dir) = sample();
        let n = lib.counts().unwrap();
        assert_eq!((n.tracks, n.artists, n.albums), (3, 1, 1));

        let p1 = lib.tracks(None, 2).unwrap();
        assert_eq!(p1.tracks.len(), 2);
        let p2 = lib.tracks(p1.next.clone(), 2).unwrap();
        assert_eq!(p2.tracks.len(), 1);
        assert!(p2.next.is_none());
        let mut titles: Vec<_> = p1
            .tracks
            .iter()
            .chain(&p2.tracks)
            .map(|t| t.title.clone())
            .collect();
        titles.sort();
        assert_eq!(titles, ["Ёлка", "Группа крови", "Закрой за мной дверь"]);

        // ё/е and the wrong keyboard layout find the same track.
        assert_eq!(lib.search("елка".into()).unwrap().tracks[0].title, "Ёлка");
        assert!(
            !lib.search("rbyj".into()).unwrap().artists.is_empty(),
            "«кино» typed in EN layout"
        );

        let id = p1.tracks[0].id;
        lib.set_like(id, true).unwrap();
        drop(lib);
        // Reopened from disk: the same library.
        let again = Library::open(dir.to_string_lossy().into_owned()).unwrap();
        assert_eq!(again.counts().unwrap().tracks, 3);
        drop(again);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
