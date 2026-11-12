//! Pictures with the transfer (D-172, owner's item 12, Q-43): the phone shows the covers the PC
//! shows — embedded, from the folder, found on the web (D-162) or chosen by hand — and never goes
//! online for them. The hub writes `covers/<hash>.<ext>` (originals from its cover store, only
//! those the phone lacks) and `covers.json` (per track: which cover its album has on the PC) into
//! the package; the phone gives each matched track's album that cover. Audio is never touched.
//! Additive to package format 1: an older app ignores both.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use sirtone_core::ports::CoverSource;
use sirtone_library::{covers, transfer};
use sirtone_transfer::{Manifest, TrackEntry, merge};

use crate::{SyncError, local_tracks};

pub const COVERS_FILE: &str = "covers.json";
pub const COVERS_DIR: &str = "covers";

/// One track and the cover its album has on the PC; matched on the phone as the merge matches
/// (by the file's hash, else by artist, title and length).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CoverEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    pub title: String,
    pub duration_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// BLAKE3 of the picture (`cover.hash`).
    pub cover: String,
    /// The track's own cover (D-173: a single, or its album has none) — not its album's.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub on_track: bool,
}

/// An artist's own picture on the PC (D-173, `artist.photo_cover_id`); the phone shows it where
/// none of their albums has a cover, as the PC does.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArtistPicture {
    pub name: String,
    pub cover: String,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct CoversFile {
    #[serde(default)]
    tracks: Vec<CoverEntry>,
    #[serde(default)]
    artists: Vec<ArtistPicture>,
}

fn as_entry(i: usize, e: &CoverEntry) -> TrackEntry {
    TrackEntry {
        key: i.to_string(),
        file: String::new(),
        size: 0,
        duration_ms: e.duration_ms,
        artist: e.artist.clone(),
        title: Some(e.title.clone()),
        album: None,
        fingerprint: None,
        hash: e.hash.clone(),
        copy: None,
        analysis: None,
    }
}

/// Track id → the picture it shows (its album's cover, else its own — D-173) and whether that
/// is its own.
fn track_covers(conn: &Connection) -> Result<HashMap<i64, (String, bool)>, SyncError> {
    let mut st = conn.prepare(
        "SELECT t.id, coalesce(ac.hash, tc.hash), ac.hash IS NULL FROM track t
           LEFT JOIN album al ON al.id = t.album_id
           LEFT JOIN cover ac ON ac.id = al.cover_id
           LEFT JOIN cover tc ON tc.id = t.cover_id
          WHERE t.state = 'ok' AND coalesce(ac.hash, tc.hash) IS NOT NULL",
    )?;
    let rows = st
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, (r.get(1)?, r.get(2)?))))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Artist name → their own picture's hash.
fn artist_pictures(conn: &Connection) -> Result<HashMap<String, String>, SyncError> {
    let mut st = conn.prepare(
        "SELECT a.name, cv.hash FROM artist a JOIN cover cv ON cv.id = a.photo_cover_id",
    )?;
    let rows = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

fn cover_hashes(conn: &Connection) -> Result<HashSet<String>, SyncError> {
    let mut st = conn.prepare("SELECT hash FROM cover")?;
    let rows = st
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    Ok(rows)
}

/// Hub side, after [`crate::narrow`]: the covers of the music that will be on the phone — the
/// tracks the package carries and those `other` (the phone's library, when readable) already
/// has — unless the phone's album shows the same picture already. Returns how many tracks get a
/// picture; 0 — nothing written.
pub fn write_covers(
    conn: &Connection,
    root: &Path,
    out: &Path,
    other: Option<&Connection>,
) -> Result<u32, SyncError> {
    let pc = track_covers(conn)?;
    let rows: HashMap<i64, transfer::TransferTrack> = transfer::tracks(conn)?
        .into_iter()
        .filter(|t| pc.contains_key(&t.id))
        .map(|t| (t.id, t))
        .collect();
    let entry = |t: &transfer::TransferTrack| CoverEntry {
        artist: t.artist.clone(),
        title: t.title.clone(),
        duration_ms: t.duration_ms,
        hash: Some(t.content_hash.clone()),
        cover: pc[&t.id].0.clone(),
        on_track: pc[&t.id].1,
    };

    // The package's tracks (keys `<uid>:<id>` of this library).
    let manifest = Manifest::read(out)?;
    let mut ids: Vec<i64> = manifest
        .tracks
        .iter()
        .filter_map(|t| t.key.rsplit_once(':')?.1.parse().ok())
        .filter(|id| rows.contains_key(id))
        .collect();
    let mut skip: HashSet<i64> = HashSet::new();
    let mut theirs: HashSet<String> = HashSet::new();
    let mut their_artists: HashMap<String, String> = HashMap::new();
    if let Some(c) = other {
        theirs = cover_hashes(c)?;
        their_artists = artist_pictures(c)?;
        // The phone's tracks and the picture each shows now.
        let there = local_tracks(c)?;
        let shows: HashMap<i64, String> = track_covers(c)?
            .into_iter()
            .map(|(k, (h, _))| (k, h))
            .collect();
        let all: Vec<(i64, CoverEntry)> = rows.values().map(|t| (t.id, entry(t))).collect();
        let entries: Vec<TrackEntry> = all.iter().map(|(_, e)| as_entry(0, e)).collect();
        for ((id, e), m) in all.iter().zip(merge::match_tracks(&entries, &there)) {
            if let merge::TrackMatch::Same(phone) = m {
                if shows.get(&phone) == Some(&e.cover) {
                    skip.insert(*id);
                } else {
                    ids.push(*id);
                }
            }
        }
    }
    // The artists of the music that will be on the phone, with their own picture on the PC.
    let names: HashSet<&str> = ids
        .iter()
        .chain(skip.iter())
        .filter_map(|id| rows[id].artist.as_deref())
        .collect();
    let artists: Vec<ArtistPicture> = artist_pictures(conn)?
        .into_iter()
        .filter(|(n, h)| names.contains(n.as_str()) && their_artists.get(n) != Some(h))
        .map(|(name, cover)| ArtistPicture { name, cover })
        .collect();
    let mut seen = HashSet::new();
    let list: Vec<CoverEntry> = ids
        .into_iter()
        .filter(|id| !skip.contains(id) && seen.insert(*id))
        .map(|id| entry(&rows[&id]))
        .collect();

    let file = out.join(COVERS_FILE);
    let dir = out.join(COVERS_DIR);
    if list.is_empty() && artists.is_empty() {
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir_all(&dir);
        return Ok(0);
    }
    let mut copied = HashSet::new();
    let hashes = list
        .iter()
        .map(|e| &e.cover)
        .chain(artists.iter().map(|a| &a.cover));
    for hash in hashes {
        if theirs.contains(hash) || !copied.insert(hash.clone()) {
            continue;
        }
        if let Some(src) = covers::original(root, hash) {
            let name = src.file_name().map(|n| n.to_owned()).unwrap_or_default();
            std::fs::create_dir_all(&dir)?;
            std::fs::copy(&src, dir.join(name))?;
        }
    }
    let n = (list.len() + artists.len()) as u32;
    let json = serde_json::to_vec(&CoversFile {
        tracks: list,
        artists,
    })
    .map_err(|e| SyncError::Io(std::io::Error::other(e)))?;
    std::fs::write(file, json)?;
    Ok(n)
}

/// Phone side, after [`crate::apply`] (the new music is imported by then): each matched track's
/// album gets the hub's picture, from the package or — when the phone has it already — from its
/// own store; a track's own cover goes to the track, an artist's picture to the artist (D-173).
/// A cover the person chose here (`cover_locked`) stays. Returns albums, tracks and artists
/// changed.
pub fn apply_covers(conn: &Connection, root: &Path, pkg: &Path) -> Result<u32, SyncError> {
    let list: CoversFile = match std::fs::read(pkg.join(COVERS_FILE)) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or_default(),
        Err(_) => return Ok(0),
    };
    let entries: Vec<TrackEntry> = list
        .tracks
        .iter()
        .enumerate()
        .map(|(i, e)| as_entry(i, e))
        .collect();
    let matches = merge::match_tracks(&entries, &local_tracks(conn)?);
    let mut ids: HashMap<String, Option<i64>> = HashMap::new();
    let mut changed = HashSet::new();
    let mut tracks = 0u32;
    for (e, m) in list.tracks.iter().zip(matches) {
        let merge::TrackMatch::Same(track) = m else {
            continue;
        };
        if e.on_track {
            if let Some(cover) = cached(&mut ids, conn, root, pkg, &e.cover)? {
                tracks += conn.execute(
                    "UPDATE track SET cover_id = ?2 WHERE id = ?1 AND cover_id IS NOT ?2",
                    [track, cover],
                )? as u32;
            }
            continue;
        }
        let Some(album) = conn
            .query_row("SELECT album_id FROM track WHERE id = ?1", [track], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .optional()?
            .flatten()
        else {
            continue;
        };
        let Some(cover) = cached(&mut ids, conn, root, pkg, &e.cover)? else {
            continue;
        };
        let n = conn.execute(
            "UPDATE album SET cover_id = ?2
              WHERE id = ?1 AND cover_locked = 0 AND cover_id IS NOT ?2",
            [album, cover],
        )?;
        if n > 0 {
            changed.insert(album);
        }
    }
    let mut artists = 0u32;
    for a in &list.artists {
        let Some(cover) = cached(&mut ids, conn, root, pkg, &a.cover)? else {
            continue;
        };
        artists += conn.execute(
            "UPDATE artist SET photo_cover_id = ?2 WHERE name = ?1 AND photo_cover_id IS NOT ?2",
            rusqlite::params![a.name, cover],
        )? as u32;
    }
    Ok(changed.len() as u32 + tracks + artists)
}

/// [`cover_id`] once per picture.
fn cached(
    ids: &mut HashMap<String, Option<i64>>,
    conn: &Connection,
    root: &Path,
    pkg: &Path,
    hash: &str,
) -> Result<Option<i64>, SyncError> {
    if let Some(c) = ids.get(hash) {
        return Ok(*c);
    }
    let c = cover_id(conn, root, pkg, hash)?;
    ids.insert(hash.to_owned(), c);
    Ok(c)
}

/// The phone's `cover.id` for a hub picture: already stored, else stored from the package.
fn cover_id(
    conn: &Connection,
    root: &Path,
    pkg: &Path,
    hash: &str,
) -> Result<Option<i64>, SyncError> {
    if let Some(id) = conn
        .query_row("SELECT id FROM cover WHERE hash = ?1", [hash], |r| r.get(0))
        .optional()?
    {
        return Ok(Some(id));
    }
    let dir = pkg.join(COVERS_DIR);
    let Some(bytes) = ["jpg", "png"]
        .iter()
        .find_map(|x| std::fs::read(dir.join(format!("{hash}.{x}"))).ok())
    else {
        return Ok(None);
    };
    // Stored by the bytes' own hash: a damaged or foreign file never poses as the hub's.
    let Ok(id) = covers::store(conn, root, &bytes, CoverSource::Folder) else {
        return Ok(None);
    };
    let stored: String =
        conn.query_row("SELECT hash FROM cover WHERE id = ?1", [id], |r| r.get(0))?;
    Ok((stored == hash).then_some(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parts, export_with, narrow};
    use sirtone_library::repo::{
        Identity, NewTrack, album_get_or_create, artist_get_or_create, track_insert,
    };
    use sirtone_transfer::Platform;

    fn png(r: u8) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbImage::from_pixel(8, 8, image::Rgb([r, 30, 40]))
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// A library at `root` with «Кино — Ёлка» (hash `h1`), its album with `cover` when given.
    fn library(root: &Path, cover: Option<&[u8]>) -> (Connection, i64) {
        std::fs::create_dir_all(root.join("Кино")).unwrap();
        std::fs::write(root.join("Кино/01 Ёлка.flac"), b"flac").unwrap();
        let mut conn = sirtone_library::open(
            &root.join(".sirtone").join("library.db"),
            sirtone_library::Medium::Fixed,
        )
        .unwrap();
        let kino = artist_get_or_create(&conn, "Кино").unwrap();
        let album = album_get_or_create(&conn, "Ёлка", kino, None).unwrap();
        track_insert(
            &mut conn,
            &NewTrack {
                rel_path: "Кино/01 Ёлка.flac".into(),
                size_bytes: 4,
                mtime_ms: 0,
                content_hash: "h1".into(),
                container: "flac".into(),
                codec: "flac".into(),
                lossless: true,
                sample_rate: Some(44100),
                channels: Some(2),
                duration_ms: 200_000,
                title: "Ёлка".into(),
                album_id: Some(album),
                disc_no: None,
                track_no: Some(1),
                year: None,
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![kino],
            },
        )
        .unwrap();
        if let Some(b) = cover {
            let id = covers::store(&conn, root, b, CoverSource::Caa).unwrap();
            covers::link_album(&conn, album, id).unwrap();
        }
        (conn, album)
    }

    fn album_cover(conn: &Connection, album: i64) -> Option<String> {
        conn.query_row(
            "SELECT cv.hash FROM album al JOIN cover cv ON cv.id = al.cover_id WHERE al.id = ?1",
            [album],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
    }

    /// The phone has the track without a picture; the PC found one on the web. The transfer
    /// carries it although no music goes, the phone shows it, and the next transfer sends nothing.
    #[test]
    fn the_phone_gets_the_pcs_picture_and_only_once() {
        let base = std::env::temp_dir().join(format!("sirtone-covers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (pc_root, phone_root, out) = (base.join("pc"), base.join("phone"), base.join("out"));
        let picture = png(200);
        let (pc, _) = library(&pc_root, Some(&picture));
        let (phone, phone_album) = library(&phone_root, None);

        let parts = Parts {
            music: true,
            playlists: true,
            taste: true,
            settings: false,
        };
        export_with(&pc, &pc_root, &out, Platform::Windows, "t", parts, false).unwrap();
        let n = narrow(&out, Some(&phone), None, None, &[]).unwrap();
        assert!(n.send.is_empty(), "the phone has the music");
        assert_eq!(write_covers(&pc, &pc_root, &out, Some(&phone)).unwrap(), 1);
        assert_eq!(std::fs::read_dir(out.join(COVERS_DIR)).unwrap().count(), 1);

        assert_eq!(apply_covers(&phone, &phone_root, &out).unwrap(), 1);
        let want = album_cover(&pc, 1);
        assert_eq!(album_cover(&phone, phone_album), want);
        assert_eq!(
            apply_covers(&phone, &phone_root, &out).unwrap(),
            0,
            "applied once"
        );

        // Next time the phone shows it already: no picture, no list.
        assert_eq!(write_covers(&pc, &pc_root, &out, Some(&phone)).unwrap(), 0);
        assert!(!out.join(COVERS_FILE).exists() && !out.join(COVERS_DIR).exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A picture the phone has already travels as a reference only; a cover chosen on the
    /// phone stays.
    #[test]
    fn known_pictures_are_not_copied_and_chosen_ones_stay() {
        let base = std::env::temp_dir().join(format!("sirtone-covers2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (pc_root, phone_root, out) = (base.join("pc"), base.join("phone"), base.join("out"));
        let picture = png(90);
        let (pc, _) = library(&pc_root, Some(&picture));
        let (phone, phone_album) = library(&phone_root, None);
        covers::store(&phone, &phone_root, &picture, CoverSource::Folder).unwrap();

        let parts = Parts {
            music: true,
            playlists: true,
            taste: true,
            settings: false,
        };
        export_with(&pc, &pc_root, &out, Platform::Windows, "t", parts, false).unwrap();
        narrow(&out, Some(&phone), None, None, &[]).unwrap();
        assert_eq!(write_covers(&pc, &pc_root, &out, Some(&phone)).unwrap(), 1);
        assert!(
            !out.join(COVERS_DIR).exists(),
            "the phone has the picture itself"
        );

        phone
            .execute(
                "UPDATE album SET cover_locked = 1 WHERE id = ?1",
                [phone_album],
            )
            .unwrap();
        assert_eq!(apply_covers(&phone, &phone_root, &out).unwrap(), 0);
        phone
            .execute(
                "UPDATE album SET cover_locked = 0 WHERE id = ?1",
                [phone_album],
            )
            .unwrap();
        assert_eq!(apply_covers(&phone, &phone_root, &out).unwrap(), 1);
        assert_eq!(album_cover(&phone, phone_album), album_cover(&pc, 1));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// D-173 on the phone: the PC's own cover of a track whose album has none, and an artist's
    /// picture, travel and land on the track and the artist.
    #[test]
    fn a_tracks_own_cover_and_an_artists_picture_travel() {
        let base = std::env::temp_dir().join(format!("sirtone-covers3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (pc_root, phone_root, out) = (base.join("pc"), base.join("phone"), base.join("out"));
        let (pc, _) = library(&pc_root, None);
        let own = covers::store(&pc, &pc_root, &png(10), CoverSource::Caa).unwrap();
        covers::link_track(&pc, 1, own).unwrap();
        let photo = covers::store(&pc, &pc_root, &png(250), CoverSource::Caa).unwrap();
        covers::link_artist(&pc, 1, photo).unwrap();
        let (phone, _) = library(&phone_root, None);

        let parts = Parts {
            music: true,
            playlists: true,
            taste: true,
            settings: false,
        };
        export_with(&pc, &pc_root, &out, Platform::Windows, "t", parts, false).unwrap();
        narrow(&out, Some(&phone), None, None, &[]).unwrap();
        assert_eq!(write_covers(&pc, &pc_root, &out, Some(&phone)).unwrap(), 2);
        assert_eq!(std::fs::read_dir(out.join(COVERS_DIR)).unwrap().count(), 2);
        assert_eq!(apply_covers(&phone, &phone_root, &out).unwrap(), 2);

        let hash = |c: &Connection, sql: &str| -> Option<String> {
            c.query_row(sql, [], |r| r.get(0)).optional().unwrap()
        };
        let track = "SELECT cv.hash FROM track t JOIN cover cv ON cv.id = t.cover_id";
        let artist = "SELECT cv.hash FROM artist a JOIN cover cv ON cv.id = a.photo_cover_id";
        assert!(hash(&phone, track).is_some());
        assert_eq!(hash(&phone, track), hash(&pc, track));
        assert_eq!(hash(&phone, artist), hash(&pc, artist));
        assert_eq!(
            sirtone_library::collection::track_cover(&phone, 1).unwrap(),
            hash(&pc, track),
            "the phone shows the track's own cover"
        );
        // Nothing left to send next time.
        assert_eq!(write_covers(&pc, &pc_root, &out, Some(&phone)).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&base);
    }
}
