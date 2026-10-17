//! Search over IPC (T2.13, FR-SRCH-001/002): «лучшее совпадение» + groups. Runs on the library's
//! read connection (S-15: p95 16.6 ms on 100 000 tracks; budget 80 ms).

use serde::{Deserialize, Serialize};
use sirtone_core::error::AppError;
use sirtone_library::collection::{AlbumRow, ArtistRow};
use sirtone_library::search::{self, Best};
use tauri::State;

use crate::library::{AlbumItem, ArtistItem, Library, TrackItem, track_item};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind", content = "item")]
pub enum BestMatch {
    Track(TrackItem),
    Artist(ArtistItem),
    Album(AlbumItem),
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistHit {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub best: Option<BestMatch>,
    pub tracks: Vec<TrackItem>,
    pub artists: Vec<ArtistItem>,
    pub albums: Vec<AlbumItem>,
    pub playlists: Vec<PlaylistHit>,
}

fn artist(a: ArtistRow) -> ArtistItem {
    ArtistItem {
        id: a.id as u32,
        name: a.name,
        tracks: a.tracks,
    }
}

fn album(a: AlbumRow) -> AlbumItem {
    AlbumItem {
        id: a.id as u32,
        title: a.title,
        artist: a.artist,
        year: a.year,
        tracks: a.tracks,
        cover: a.cover,
    }
}

#[tauri::command(async)]
#[specta::specta]
pub fn library_search(lib: State<'_, Library>, query: String) -> Result<SearchResults, AppError> {
    let r = lib.with_read(|c| search::search(c, &query))?;
    Ok(SearchResults {
        best: r.best.map(|b| match b {
            Best::Track(t) => BestMatch::Track(track_item(t)),
            Best::Artist(a) => BestMatch::Artist(artist(a)),
            Best::Album(a) => BestMatch::Album(album(a)),
        }),
        tracks: r.tracks.into_iter().map(track_item).collect(),
        artists: r.artists.into_iter().map(artist).collect(),
        albums: r.albums.into_iter().map(album).collect(),
        playlists: r
            .playlists
            .into_iter()
            .map(|(id, name)| PlaylistHit {
                id: id as u32,
                name,
            })
            .collect(),
    })
}
