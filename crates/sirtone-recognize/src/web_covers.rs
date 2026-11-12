//! Covers from the web when the album has none (D-162, FR-LOOK-016): Deezer and iTunes search.
//! Pure parts only — the request paths and the choice of the answer; the calls go through
//! `sirtone-net` (allowlist, rate limits, cache, offline switch). A cover is taken only when the
//! artist and the album match closely (the principle «no certainty — no change»).

use serde_json::Value;
use sirtone_core::text::norm;

use crate::identify::similarity;

const MIN_SIMILARITY: f64 = 0.8;
/// A person's photo needs an almost exact name (D-173).
const MIN_ARTIST: f64 = 0.9;

fn same_album(found: &str, wanted: &str) -> bool {
    if similarity(found, wanted) >= MIN_SIMILARITY {
        return true;
    }
    // «Album (Deluxe Edition)» is still the album.
    match (norm(found), norm(wanted)) {
        (Some(f), Some(w)) => w.len() >= 4 && f.starts_with(&w),
        _ => false,
    }
}

/// Deezer: `search/album?q=artist:"…" album:"…"` under `https://api.deezer.com/`.
pub fn deezer_path(artist: &str, album: &str, encode: impl Fn(&str) -> String) -> String {
    let q = format!("artist:\"{artist}\" album:\"{album}\"");
    format!("search/album?q={}&limit=5", encode(&q))
}

/// The best match's cover as a path under the Deezer image host.
pub fn deezer_pick(json: &[u8], artist: &str, album: &str) -> Option<String> {
    let v: Value = serde_json::from_slice(json).ok()?;
    v.get("data")?.as_array()?.iter().find_map(|d| {
        let by = d.get("artist")?.get("name")?.as_str()?;
        let title = d.get("title")?.as_str()?;
        if similarity(by, artist) < MIN_SIMILARITY || !same_album(title, album) {
            return None;
        }
        let url = d.get("cover_big")?.as_str()?;
        image_path(url, "dzcdn.net/")
    })
}

/// iTunes: `search?term=…&entity=album` under `https://itunes.apple.com/`.
pub fn itunes_path(artist: &str, album: &str, encode: impl Fn(&str) -> String) -> String {
    format!(
        "search?term={}&entity=album&limit=5",
        encode(&format!("{artist} {album}"))
    )
}

/// The best match's cover (600 px) as a path under the Apple image host.
pub fn itunes_pick(json: &[u8], artist: &str, album: &str) -> Option<String> {
    let v: Value = serde_json::from_slice(json).ok()?;
    v.get("results")?.as_array()?.iter().find_map(|d| {
        let by = d.get("artistName")?.as_str()?;
        let title = d.get("collectionName")?.as_str()?;
        if similarity(by, artist) < MIN_SIMILARITY || !same_album(title, album) {
            return None;
        }
        let url = d.get("artworkUrl100")?.as_str()?;
        image_path(url, "mzstatic.com/").map(|p| p.replace("100x100bb", "600x600bb"))
    })
}

/// Deezer artist search: `search/artist?q=…` (D-173).
pub fn deezer_artist_path(artist: &str, encode: impl Fn(&str) -> String) -> String {
    format!(
        "search/artist?q={}&limit=5",
        encode(&format!("artist:\"{artist}\""))
    )
}

/// The photo (`picture_xl`) of the artist whose name is almost exactly the wanted one.
pub fn deezer_artist_pick(json: &[u8], artist: &str) -> Option<String> {
    let v: Value = serde_json::from_slice(json).ok()?;
    v.get("data")?.as_array()?.iter().find_map(|d| {
        let name = d.get("name")?.as_str()?;
        if similarity(name, artist) < MIN_ARTIST {
            return None;
        }
        // Deezer's «no picture» placeholder has an empty hash in its path (`/artist//`).
        let path = image_path(d.get("picture_xl")?.as_str()?, "dzcdn.net/")?;
        (!path.contains("//")).then_some(path)
    })
}

/// Deezer track search: `search/track?q=artist:"…" track:"…"` (D-173).
pub fn deezer_track_path(artist: &str, title: &str, encode: impl Fn(&str) -> String) -> String {
    let q = format!("artist:\"{artist}\" track:\"{title}\"");
    format!("search/track?q={}&limit=5", encode(&q))
}

/// The cover of the album of the track whose artist and title match closely.
pub fn deezer_track_pick(json: &[u8], artist: &str, title: &str) -> Option<String> {
    let v: Value = serde_json::from_slice(json).ok()?;
    v.get("data")?.as_array()?.iter().find_map(|d| {
        let by = d.get("artist")?.get("name")?.as_str()?;
        let name = d.get("title_short").or_else(|| d.get("title"))?.as_str()?;
        if similarity(by, artist) < MIN_SIMILARITY || similarity(name, title) < MIN_SIMILARITY {
            return None;
        }
        image_path(d.get("album")?.get("cover_big")?.as_str()?, "dzcdn.net/")
    })
}

/// «https://any-subdomain.<site>/path» → `path` (the host is fixed by `sirtone-net`).
fn image_path(url: &str, site: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_once('/')?;
    (host.ends_with(site.trim_end_matches('/')) && !path.contains("..") && !path.is_empty())
        .then(|| path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deezer_takes_the_matching_album() {
        let json = r#"{"data":[
          {"title":"Другой","artist":{"name":"Кино"},"cover_big":"https://cdn-images.dzcdn.net/images/cover/aaa/500x500-000000-80-0-0.jpg"},
          {"title":"Группа крови (Remastered)","artist":{"name":"Кино"},"cover_big":"https://e-cdns-images.dzcdn.net/images/cover/bbb/500x500-000000-80-0-0.jpg"}]}"#;
        assert_eq!(
            deezer_pick(json.as_bytes(), "Кино", "Группа крови").as_deref(),
            Some("images/cover/bbb/500x500-000000-80-0-0.jpg")
        );
        assert_eq!(deezer_pick(json.as_bytes(), "Ария", "Группа крови"), None);
    }

    #[test]
    fn itunes_gives_a_600_px_path() {
        let json = r#"{"results":[{"collectionName":"Jubilee","artistName":"Jubilee","artworkUrl100":"https://is3-ssl.mzstatic.com/image/thumb/Music/x/y/source/100x100bb.jpg"}]}"#;
        assert_eq!(
            itunes_pick(json.as_bytes(), "Jubilee", "Jubilee").as_deref(),
            Some("image/thumb/Music/x/y/source/600x600bb.jpg")
        );
        assert_eq!(itunes_pick(b"nope", "a", "b"), None);
    }

    #[test]
    fn artist_photo_needs_the_same_name() {
        let json = r#"{"data":[
          {"name":"Кино-2","picture_xl":"https://e-cdns-images.dzcdn.net/images/artist/aaa/1000x1000-000000-80-0-0.jpg"},
          {"name":"Кино","picture_xl":"https://cdn-images.dzcdn.net/images/artist/bbb/1000x1000-000000-80-0-0.jpg"}]}"#;
        assert_eq!(
            deezer_artist_pick(json.as_bytes(), "Кино").as_deref(),
            Some("images/artist/bbb/1000x1000-000000-80-0-0.jpg")
        );
        assert_eq!(deezer_artist_pick(json.as_bytes(), "Ария"), None);
    }

    #[test]
    fn artist_placeholder_photo_is_refused() {
        let json = r#"{"data":[{"name":"Кино","picture_xl":"https://cdn-images.dzcdn.net/images/artist//1000x1000-000000-80-0-0.jpg"}]}"#;
        assert_eq!(deezer_artist_pick(json.as_bytes(), "Кино"), None);
    }

    #[test]
    fn track_cover_is_the_found_tracks_album() {
        let json = r#"{"data":[
          {"title":"Другая","title_short":"Другая","artist":{"name":"Кино"},"album":{"cover_big":"https://cdn-images.dzcdn.net/images/cover/aaa/500x500-000000-80-0-0.jpg"}},
          {"title":"Звезда по имени Солнце (Remastered)","title_short":"Звезда по имени Солнце","artist":{"name":"Кино"},"album":{"cover_big":"https://cdn-images.dzcdn.net/images/cover/bbb/500x500-000000-80-0-0.jpg"}}]}"#;
        assert_eq!(
            deezer_track_pick(json.as_bytes(), "Кино", "Звезда по имени Солнце").as_deref(),
            Some("images/cover/bbb/500x500-000000-80-0-0.jpg")
        );
        assert_eq!(
            deezer_track_pick(json.as_bytes(), "Ария", "Звезда по имени Солнце"),
            None
        );
        assert_eq!(deezer_track_pick(b"{}", "Кино", "x"), None);
    }

    #[test]
    fn request_paths_are_encoded_queries() {
        let enc = |q: &str| q.replace(' ', "+");
        assert_eq!(
            deezer_artist_path("Кино", enc),
            "search/artist?q=artist:\"Кино\"&limit=5"
        );
        assert!(
            deezer_track_path("A", "B", enc).starts_with("search/track?q=artist:\"A\"+track:\"B\"")
        );
    }

    #[test]
    fn other_hosts_are_refused() {
        assert_eq!(image_path("https://evil.example/x.jpg", "dzcdn.net/"), None);
        assert_eq!(image_path("http://cdn.dzcdn.net/x.jpg", "dzcdn.net/"), None);
    }
}
