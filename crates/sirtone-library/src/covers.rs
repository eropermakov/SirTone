//! Cover store (T1.26, docs/03 §8.1, §5.6): originals in `.sirtone/covers/orig/<hash>.<ext>`,
//! sized WebP variants in `.sirtone/covers/<size>/<hash>.webp`, generated lazily on request.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use sirtone_core::ports::CoverSource;

use crate::db::now_ms;
use crate::error::LibraryError;
use crate::import::SERVICE_DIR;
use crate::repo::AlbumId;

/// Largest cover accepted (≈ 6300 × 6300): limits memory when it is decoded for a variant.
pub const MAX_PIXELS: u64 = 40_000_000;

/// Sizes the protocol serves (docs/03 §5.6).
pub const SIZES: &[u32] = &[64, 128, 256, 512, 1024];

fn covers_dir(root: &Path) -> PathBuf {
    root.join(SERVICE_DIR).join("covers")
}

fn ext_of(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG") {
        "png"
    } else {
        "jpg"
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        // Another request produced the same file first: identical content, keep theirs.
        Err(_) if path.exists() => {
            let _ = fs::remove_file(&tmp);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Stores the original once (deduplicated by BLAKE3 of the bytes) and returns `cover.id`.
pub fn store(
    conn: &Connection,
    root: &Path,
    bytes: &[u8],
    source: CoverSource,
) -> Result<i64, LibraryError> {
    let hash = blake3::hash(bytes).to_hex().to_string();
    if let Some(id) = conn
        .query_row("SELECT id FROM cover WHERE hash = ?1", [&hash], |r| {
            r.get(0)
        })
        .optional()?
    {
        return Ok(id);
    }
    let (w, h) = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .ok_or_else(|| LibraryError::Invalid("not a decodable image".into()))?;
    // docs/08 §6: a tiny file can declare enormous dimensions; never decode those.
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(LibraryError::Invalid("cover too large".into()));
    }
    let path = covers_dir(root)
        .join("orig")
        .join(format!("{hash}.{}", ext_of(bytes)));
    write_atomic(&path, bytes)?;
    conn.execute(
        "INSERT INTO cover(hash, source, width, height, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![hash, source.as_str(), w, h, now_ms()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Gives the album this cover unless it already has one or the user chose one (`cover_locked`).
pub fn link_album(conn: &Connection, album: AlbumId, cover: i64) -> Result<(), LibraryError> {
    conn.execute(
        "UPDATE album SET cover_id = ?2 WHERE id = ?1 AND cover_id IS NULL AND cover_locked = 0",
        params![album, cover],
    )?;
    Ok(())
}

pub fn album_has_cover(conn: &Connection, album: AlbumId) -> Result<bool, LibraryError> {
    Ok(conn.query_row(
        "SELECT cover_id IS NOT NULL FROM album WHERE id = ?1",
        [album],
        |r| r.get(0),
    )?)
}

/// Albums imported before covers existed (or whose first track had none) get one from the
/// embedded art of their files in the library. Runs at startup; at most `limit` albums per call.
pub fn backfill(
    conn: &Connection,
    root: &Path,
    reader: &dyn sirtone_core::ports::CoverReader,
    limit: usize,
) -> Result<usize, LibraryError> {
    let mut st = conn.prepare(
        "SELECT al.id, t.rel_path FROM album al JOIN track t ON t.album_id = al.id
         WHERE al.cover_id IS NULL AND al.cover_locked = 0 AND t.state = 'ok'
         GROUP BY al.id LIMIT ?1",
    )?;
    let todo: Vec<(AlbumId, String)> = st
        .query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut done = 0;
    for (album, rel) in todo {
        let file = rel.split('/').fold(root.to_path_buf(), |p, c| p.join(c));
        if let Some((bytes, source)) = reader.read(&file, None)
            && let Ok(cover) = store(conn, root, &bytes, source)
        {
            link_album(conn, album, cover)?;
            done += 1;
        }
    }
    Ok(done)
}

fn valid_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Path of the `size` px WebP for `hash`, generated on first request (square crop, Lanczos3,
/// quality 80). Only known hashes and sizes are accepted — nothing else on disk is reachable.
pub fn variant(root: &Path, hash: &str, size: u32) -> Result<PathBuf, LibraryError> {
    if !valid_hash(hash) || !SIZES.contains(&size) {
        return Err(LibraryError::Invalid("bad cover request".into()));
    }
    let out = covers_dir(root)
        .join(size.to_string())
        .join(format!("{hash}.webp"));
    if out.exists() {
        return Ok(out);
    }
    let orig_dir = covers_dir(root).join("orig");
    let orig = ["jpg", "png"]
        .iter()
        .map(|e| orig_dir.join(format!("{hash}.{e}")))
        .find(|p| p.exists())
        .ok_or_else(|| LibraryError::Invalid("no such cover".into()))?;
    let img = image::ImageReader::open(&orig)?
        .with_guessed_format()?
        .decode()
        .map_err(|e| LibraryError::Invalid(e.to_string()))?;
    let thumb = img
        .resize_to_fill(size, size, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    let encoded =
        webp::Encoder::from_rgb(thumb.as_raw(), thumb.width(), thumb.height()).encode(80.0);
    write_atomic(&out, &encoded)?;
    Ok(out)
}

/// A 512 px JPEG of the cover for Windows media controls (docs/07 §3): SMTC thumbnails come from
/// a file; JPEG is what every widget reading them understands. Cached by hash in `dir`.
pub fn smtc_jpeg(root: &Path, hash: &str, dir: &Path) -> Result<PathBuf, LibraryError> {
    if !valid_hash(hash) {
        return Err(LibraryError::Invalid("bad cover request".into()));
    }
    let out = dir.join(format!("{hash}.jpg"));
    if out.exists() {
        return Ok(out);
    }
    let webp = variant(root, hash, 512)?;
    let img = image::ImageReader::open(&webp)?
        .with_guessed_format()?
        .decode()
        .map_err(|e| LibraryError::Invalid(e.to_string()))?
        .to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 85)
        .encode_image(&img)
        .map_err(|e| LibraryError::Invalid(e.to_string()))?;
    fs::create_dir_all(dir)?;
    write_atomic(&out, &bytes)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::repo::{album_get_or_create, artist_get_or_create};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn store_dedupes_links_and_serves_sized_webp() {
        let dir = std::env::temp_dir().join(format!("sirtone-covers-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let c = open_in_memory().unwrap();
        let a = artist_get_or_create(&c, "Би-2").unwrap();
        let album = album_get_or_create(&c, "Иномарки", a, None).unwrap();
        let bytes = png(600, 500);
        let id = store(&c, &dir, &bytes, CoverSource::Embedded).unwrap();
        assert_eq!(
            store(&c, &dir, &bytes, CoverSource::Folder).unwrap(),
            id,
            "deduplicated"
        );
        assert!(!album_has_cover(&c, album).unwrap());
        link_album(&c, album, id).unwrap();
        assert!(album_has_cover(&c, album).unwrap());
        let other = store(&c, &dir, &png(100, 100), CoverSource::Folder).unwrap();
        link_album(&c, album, other).unwrap();
        let kept: i64 = c
            .query_row("SELECT cover_id FROM album WHERE id = ?1", [album], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kept, id, "first cover is kept");

        let hash: String = c
            .query_row("SELECT hash FROM cover WHERE id = ?1", [id], |r| r.get(0))
            .unwrap();
        let p = variant(&dir, &hash, 128).unwrap();
        let webp = fs::read(&p).unwrap();
        assert_eq!(&webp[0..4], b"RIFF");
        assert_eq!(&webp[8..12], b"WEBP");
        let dims = image::ImageReader::open(&p)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap();
        assert_eq!(dims, (128, 128));
        assert!(variant(&dir, &hash, 100).is_err(), "unknown size");
        assert!(variant(&dir, "../../etc", 128).is_err(), "path tricks");
        assert!(store(&c, &dir, b"not an image", CoverSource::Folder).is_err());
        // A small PNG that declares 8000×8000 pixels is refused before any decoding.
        let huge = png(8000, 8000);
        assert!(store(&c, &dir, &huge, CoverSource::Folder).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
