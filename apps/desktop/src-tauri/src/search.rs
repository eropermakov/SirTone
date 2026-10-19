//! Search over IPC (T2.13, FR-SRCH-001/002): «лучшее совпадение» + groups. Runs on the library's
//! read connection (S-15: p95 16.6 ms on 100 000 tracks; budget 80 ms). T3.16: a smart query
//! («что-то грустное и медленное», FR-SRCH-004) also gets «По смыслу» — the tracks and the
//! wave query of «Играть этот вайб»; nothing found → the closest artists (FR-SRCH-005).

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sirtone_core::error::AppError;
use sirtone_library::collection::{AlbumRow, ArtistRow};
use sirtone_library::search::{self, Best};
use sirtone_library::semantic::{Facet, Lexicon, rules_json};
use tauri::State;

use crate::library::{AlbumItem, ArtistItem, Library, TrackItem, track_item};
use crate::wave::WaveQueryIpc;

/// «По смыслу»: what was understood (i18n keys under `search.vibe`), matching tracks and the
/// wave query for «Играть этот вайб».
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VibeHit {
    /// `mood.sad`, `character.calm`, `lang.none`, `activity.sleep`, `tempo.slow` / `tempo.fast`.
    pub chips: Vec<String>,
    pub tracks: Vec<TrackItem>,
    pub query: WaveQueryIpc,
}

/// Tracks under «По смыслу».
const VIBE_TRACKS: u32 = 50;

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
    pub vibe: Option<VibeHit>,
    /// Nothing found: the library's closest artists.
    pub suggestions: Vec<ArtistItem>,
}

fn lexicon() -> Option<&'static Lexicon> {
    static LEX: OnceLock<Option<Lexicon>> = OnceLock::new();
    LEX.get_or_init(|| {
        Lexicon::builtin()
            .map_err(|e| tracing::error!(target: "sirtone::search", error = %e, "lexicon"))
            .ok()
    })
    .as_ref()
}

fn vibe_query(facets: &[Facet]) -> (Vec<String>, WaveQueryIpc) {
    let mut q = WaveQueryIpc::default();
    let mut chips = Vec::new();
    for f in facets {
        match f {
            Facet::Mood(m) => {
                q.mood.push(m.clone());
                chips.push(format!("mood.{m}"));
            }
            Facet::Character(c) => {
                q.character.push(c.clone());
                chips.push(format!("character.{c}"));
            }
            Facet::Lang(l) => {
                q.language.push(l.clone());
                chips.push(format!("lang.{l}"));
            }
            Facet::Activity(a) => {
                q.activity.push(a.clone());
                chips.push(format!("activity.{a}"));
            }
            Facet::Tempo(lo, hi) => {
                q.tempo = Some([*lo, *hi]);
                chips.push(
                    if *lo <= 0.0 {
                        "tempo.slow"
                    } else {
                        "tempo.fast"
                    }
                    .into(),
                );
            }
        }
    }
    (chips, q)
}

fn artist(a: ArtistRow) -> ArtistItem {
    ArtistItem {
        id: a.id as u32,
        name: a.name,
        tracks: a.tracks,
        added_at: a.added_at as f64,
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
        added_at: a.added_at as f64,
        art: sirtone_library::art::UNKNOWN_GENRE << 4,
    }
}

#[tauri::command(async)]
#[specta::specta]
pub fn library_search(lib: State<'_, Library>, query: String) -> Result<SearchResults, AppError> {
    let r = lib.with_read(|c| search::search(c, &query))?;
    let vibe = match lexicon().map(|l| l.parse(&query)).filter(|f| !f.is_empty()) {
        Some(facets) => {
            let rules = sirtone_library::smart::Rules::parse(&rules_json(&facets, VIBE_TRACKS))?;
            let now = sirtone_library::db::now_ms();
            let tracks = lib.with_read(|c| sirtone_library::smart::evaluate(c, &rules, now))?;
            let (chips, query) = vibe_query(&facets);
            Some(VibeHit {
                chips,
                tracks: tracks.into_iter().map(track_item).collect(),
                query,
            })
        }
        None => None,
    };
    let nothing = r.best.is_none()
        && r.tracks.is_empty()
        && r.artists.is_empty()
        && r.albums.is_empty()
        && r.playlists.is_empty()
        && vibe.as_ref().is_none_or(|v| v.tracks.is_empty());
    let suggestions = if nothing {
        lib.with_read(|c| search::suggest_artists(c, &query, 5))?
            .into_iter()
            .map(artist)
            .collect()
    } else {
        Vec::new()
    };
    let mut out = SearchResults {
        vibe,
        suggestions,
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
    };
    // Placeholders of albums without a cover (T3.20).
    let mut ids: Vec<i64> = out
        .albums
        .iter()
        .filter(|a| a.cover.is_none())
        .map(|a| i64::from(a.id))
        .collect();
    if let Some(BestMatch::Album(a)) = &out.best
        && a.cover.is_none()
    {
        ids.push(i64::from(a.id));
    }
    if !ids.is_empty() {
        let art = lib.with_read(|c| sirtone_library::art::album_hints(c, &ids))?;
        let fill = |a: &mut AlbumItem| {
            if let Some(v) = art.get(&i64::from(a.id)) {
                a.art = *v;
            }
        };
        out.albums.iter_mut().for_each(fill);
        if let Some(BestMatch::Album(a)) = &mut out.best {
            fill(a);
        }
    }
    Ok(out)
}
