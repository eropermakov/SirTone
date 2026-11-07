//! What the phone's library screens list (M4.4, S02/S08): albums and artists in pages, their
//! tracks, «Любимое», «Неизвестные», manual playlists, the file to play and cover pictures.
//! The same queries as the PC (`sirtone-library::collection`, `playlists`, `covers`).

use std::path::Path;

use sirtone_library::{collection, covers, playlists};

use crate::CoreError;
use crate::library::{Library, TrackItem, item};

/// Keyset cursor of the albums and artists pages; opaque to the app.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PageCursor {
    pub key: String,
    pub id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AlbumItem {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub year: Option<i32>,
    pub tracks: u32,
    /// Cover hash for [`Library::cover_path`].
    pub cover: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct AlbumsPage {
    pub albums: Vec<AlbumItem>,
    pub next: Option<PageCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArtistItem {
    pub id: i64,
    pub name: String,
    pub tracks: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ArtistsPage {
    pub artists: Vec<ArtistItem>,
    pub next: Option<PageCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PlaylistItem {
    pub id: i64,
    pub name: String,
    pub tracks: u32,
    pub duration_ms: u64,
    /// Up to four album cover hashes (a 2×2 mosaic when four).
    pub covers: Vec<String>,
}

fn cursor(c: Option<PageCursor>) -> Option<collection::Cursor> {
    c.map(|c| collection::Cursor {
        key: c.key,
        id: c.id,
    })
}

fn page(c: Option<collection::Cursor>) -> Option<PageCursor> {
    c.map(|c| PageCursor {
        key: c.key,
        id: c.id,
    })
}

#[uniffi::export]
impl Library {
    /// Albums by title, one page (`limit` ≤ 200).
    pub fn albums(&self, after: Option<PageCursor>, limit: u32) -> Result<AlbumsPage, CoreError> {
        let (rows, next) =
            self.with(|c| collection::albums_page(c, cursor(after).as_ref(), limit.min(200)))?;
        Ok(AlbumsPage {
            albums: rows
                .into_iter()
                .map(|a| AlbumItem {
                    id: a.id,
                    title: a.title,
                    artist: a.artist,
                    year: a.year,
                    tracks: a.tracks,
                    cover: a.cover,
                })
                .collect(),
            next: page(next),
        })
    }

    /// Artists by name, one page (`limit` ≤ 200).
    pub fn artists(&self, after: Option<PageCursor>, limit: u32) -> Result<ArtistsPage, CoreError> {
        let (rows, next) =
            self.with(|c| collection::artists_page(c, cursor(after).as_ref(), limit.min(200)))?;
        Ok(ArtistsPage {
            artists: rows
                .into_iter()
                .map(|a| ArtistItem {
                    id: a.id,
                    name: a.name,
                    tracks: a.tracks,
                })
                .collect(),
            next: page(next),
        })
    }

    /// An album's tracks in disc and track order (S08).
    pub fn album_tracks(&self, album_id: i64) -> Result<Vec<TrackItem>, CoreError> {
        let rows = self.with(|c| collection::album_tracks(c, album_id))?;
        Ok(rows.into_iter().map(item).collect())
    }

    /// An artist's tracks by album year and number (S08).
    pub fn artist_tracks(&self, artist_id: i64) -> Result<Vec<TrackItem>, CoreError> {
        let rows = self.with(|c| collection::artist_tracks(c, artist_id))?;
        Ok(rows.into_iter().map(item).collect())
    }

    /// «Любимое»: latest ♡ first.
    pub fn liked_tracks(&self, limit: u32) -> Result<Vec<TrackItem>, CoreError> {
        let rows = self.with(|c| collection::liked_tracks(c, limit.min(5_000)))?;
        Ok(rows.into_iter().map(item).collect())
    }

    /// «Неизвестные»: newest first.
    pub fn unknown_tracks(&self, limit: u32) -> Result<Vec<TrackItem>, CoreError> {
        let rows = self.with(|c| collection::unknown_tracks(c, limit.min(5_000)))?;
        Ok(rows.into_iter().map(item).collect())
    }

    /// Tracks by id in the given order (the queue, the wave's picks); unplayable ids are left out.
    pub fn tracks_by_ids(&self, ids: Vec<i64>) -> Result<Vec<TrackItem>, CoreError> {
        let rows = self.with(|c| collection::tracks_by_ids(c, &ids))?;
        Ok(rows.into_iter().map(item).collect())
    }

    /// Whether ♡ is on.
    pub fn is_liked(&self, track_id: i64) -> Result<bool, CoreError> {
        Ok(self
            .with(|c| sirtone_library::feedback::track_state(c, track_id))?
            .liked)
    }

    /// The file to play: an absolute path inside the library root.
    pub fn track_path(&self, track_id: i64) -> Result<String, CoreError> {
        let rows = self.with(|c| collection::tracks_by_ids(c, &[track_id]))?;
        let t = rows.into_iter().next().ok_or(CoreError::Failed {
            code: "library.no_track".into(),
        })?;
        Ok(Path::new(&self.root())
            .join(&t.rel_path)
            .to_string_lossy()
            .into_owned())
    }

    /// The picture file of a cover hash at `size` px (64, 128, 256, 512 or 1024; WebP),
    /// made on first request. `None` when the cover is gone.
    pub fn cover_path(&self, hash: String, size: u32) -> Option<String> {
        let size = *covers::SIZES.iter().find(|s| **s >= size).unwrap_or(&1024);
        covers::variant(Path::new(&self.root()), &hash, size)
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// The cover hash of a track's album (mini-player, «Сейчас играет»).
    pub fn track_cover(&self, track_id: i64) -> Option<String> {
        self.with(|c| collection::track_cover(c, track_id))
            .ok()
            .flatten()
    }

    /// Manual playlists (S02 «Плейлисты»); smart ones are the PC's.
    pub fn playlists(&self) -> Result<Vec<PlaylistItem>, CoreError> {
        let rows = self.with(|c| playlists::list(c))?;
        Ok(rows
            .into_iter()
            .filter(|p| !p.smart)
            .map(|p| PlaylistItem {
                id: p.id,
                name: p.name,
                tracks: p.tracks,
                duration_ms: p.duration_ms,
                covers: p.covers,
            })
            .collect())
    }

    pub fn playlist_tracks(&self, playlist_id: i64) -> Result<Vec<TrackItem>, CoreError> {
        let rows = self.with(|c| playlists::tracks(c, playlist_id))?;
        Ok(rows.into_iter().map(|p| item(p.track)).collect())
    }

    /// A new manual playlist; returns its id.
    pub fn create_playlist(&self, name: String) -> Result<i64, CoreError> {
        Ok(self.with(|c| playlists::create(c, &name))?)
    }

    /// Adds tracks to the end; returns how many were added.
    pub fn add_to_playlist(&self, playlist_id: i64, track_ids: Vec<i64>) -> Result<u32, CoreError> {
        Ok(self.with(|c| playlists::add(c, playlist_id, &track_ids))?)
    }
}

#[cfg(test)]
mod tests {
    use crate::library::tests::sample;

    #[test]
    fn lists_albums_artists_and_their_tracks() {
        let (lib, dir) = sample();
        let albums = lib.albums(None, 50).unwrap();
        assert_eq!(albums.albums.len(), 1);
        assert_eq!(albums.albums[0].title, "Группа крови");
        assert_eq!(albums.albums[0].tracks, 3);
        let tracks = lib.album_tracks(albums.albums[0].id).unwrap();
        assert_eq!(tracks.len(), 3);
        assert_eq!(tracks[0].title, "Группа крови");
        assert_eq!(tracks[0].album_id, Some(albums.albums[0].id));
        let artists = lib.artists(None, 50).unwrap();
        assert_eq!(artists.artists[0].name, "Кино");
        assert_eq!(lib.artist_tracks(artists.artists[0].id).unwrap().len(), 3);
        let path = lib.track_path(tracks[0].id).unwrap();
        assert!(path.starts_with(&dir.to_string_lossy().into_owned()));
        assert!(path.ends_with("01 Группа крови.flac"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn liked_follows_the_latest_mark() {
        let (lib, dir) = sample();
        let t = lib.tracks(None, 10).unwrap().tracks;
        lib.set_like(t[0].id, true).unwrap();
        lib.set_like(t[1].id, true).unwrap();
        lib.set_like(t[0].id, false).unwrap();
        let liked = lib.liked_tracks(10).unwrap();
        assert_eq!(liked.iter().map(|x| x.id).collect::<Vec<_>>(), [t[1].id]);
        assert!(lib.is_liked(t[1].id).unwrap());
        assert!(!lib.is_liked(t[0].id).unwrap());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn manual_playlists_round_trip() {
        let (lib, dir) = sample();
        let t = lib.tracks(None, 10).unwrap().tracks;
        let id = lib.create_playlist("В дорогу".into()).unwrap();
        assert_eq!(lib.add_to_playlist(id, vec![t[2].id, t[0].id]).unwrap(), 2);
        let lists = lib.playlists().unwrap();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].tracks, 2);
        let ids: Vec<i64> = lib
            .playlist_tracks(id)
            .unwrap()
            .iter()
            .map(|x| x.id)
            .collect();
        assert_eq!(ids, [t[2].id, t[0].id]);
        assert_eq!(lib.tracks_by_ids(vec![t[1].id, 999_999]).unwrap().len(), 1);
        assert_eq!(lib.cover_path("nohash".into(), 256), None);
        assert_eq!(lib.track_cover(t[0].id), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
