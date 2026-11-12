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
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct CoversFile {
    tracks: Vec<CoverEntry>,
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

/// Track id → the hash of its album's cover.
fn track_covers(conn: &Connection) -> Result<HashMap<i64, String>, SyncError> {
    let mut st = conn.prepare(
        "SELECT t.id, cv.hash FROM track t
           JOIN album al ON al.id = t.album_id
           JOIN cover cv ON cv.id = al.cover_id
          WHERE t.state = 'ok'",
    )?;
    let rows = st
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
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
        cover: pc[&t.id].clone(),
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
    if let Some(c) = other {
        theirs = cover_hashes(c)?;
        // The phone's tracks and the picture each shows now.
        let there = local_tracks(c)?;
        let shows: HashMap<i64, String> = track_covers(c)?;
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
    let mut seen = HashSet::new();
    let list: Vec<CoverEntry> = ids
        .into_iter()
        .filter(|id| !skip.contains(id) && seen.insert(*id))
        .map(|id| entry(&rows[&id]))
        .collect();

    let file = out.join(COVERS_FILE);
    let dir = out.join(COVERS_DIR);
    if list.is_empty() {
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_dir_all(&dir);
        return Ok(0);
    }
    let mut copied = HashSet::new();
    for e in &list {
        if theirs.contains(&e.cover) || !copied.insert(e.cover.clone()) {
            continue;
        }
        if let Some(src) = covers::original(root, &e.cover) {
            let name = src.file_name().map(|n| n.to_owned()).unwrap_or_default();
            std::fs::create_dir_all(&dir)?;
            std::fs::copy(&src, dir.join(name))?;
        }
    }
    let n = list.len() as u32;
    let json = serde_json::to_vec(&CoversFile { tracks: list })
        .map_err(|e| SyncError::Io(std::io::Error::other(e)))?;
    std::fs::write(file, json)?;
    Ok(n)
}

/// Phone side, after [`crate::apply`] (the new music is imported by then): each matched track's
/// album gets the hub's picture, from the package or — when the phone has it already — from its
/// own store. A cover the person chose here (`cover_locked`) stays. Returns albums changed.
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
    for (e, m) in list.tracks.iter().zip(matches) {
        let merge::TrackMatch::Same(track) = m else {
            continue;
        };
        let Some(album) = conn
            .query_row("SELECT album_id FROM track WHERE id = ?1", [track], |r| {
                r.get::<_, Option<i64>>(0)
            })
            .optional()?
            .flatten()
        else {
            continue;
        };
        let cover = match ids.get(&e.cover) {
            Some(c) => *c,
            None => {
                let c = cover_id(conn, root, pkg, &e.cover)?;
                ids.insert(e.cover.clone(), c);
                c
            }
        };
        let Some(cover) = cover else { continue };
        let n = conn.execute(
            "UPDATE album SET cover_id = ?2
              WHERE id = ?1 AND cover_locked = 0 AND cover_id IS NOT ?2",
            [album, cover],
        )?;
        if n > 0 {
            changed.insert(album);
        }
    }
    Ok(changed.len() as u32)
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
}
