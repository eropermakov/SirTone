//! Economy copies on the phone (D-146, M8.1): «Хранилище» numbers, copies made in batches (the
//! app loops and shows progress), «Удалить оригиналы, оставить копии», and import straight into
//! a chosen quality. Encoding runs without holding the library lock, so the app stays usable.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use sirtone_audio::encode::{self, Level, Tags};
use sirtone_library::copies;

use crate::CoreError;
use crate::import::ImportSummary;
use crate::library::Library;

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct StorageInfo {
    pub original_bytes: u64,
    pub copy_bytes: u64,
    /// Tracks with both the original and a copy.
    pub both: u64,
    pub copy_only: u64,
    pub original_only: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct CopyBatch {
    pub made: u32,
    /// Tracks the encoder could not read (they keep their original).
    pub failed: u32,
    /// More tracks are waiting: call again with `after` = `next_after`.
    pub more: bool,
    /// The last track this batch looked at (a failed one is not offered again).
    pub next_after: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Freed {
    pub tracks: u32,
    pub bytes: u64,
}

fn level(s: &str) -> Result<Level, CoreError> {
    Level::parse(s).ok_or(CoreError::Failed {
        code: "copies.level".into(),
    })
}

/// What the copy carries so it stands on its own on another device (the PC, a reinstall).
pub(crate) fn tags_of(j: &copies::CopyJob) -> Tags {
    Tags {
        title: Some(j.title.clone()),
        artist: Some(j.artists.clone()).filter(|a| !a.is_empty()),
        album: j.album.clone(),
        album_artist: j.album_artist.clone(),
        track_no: j.track_no,
        disc_no: j.disc_no,
        year: j.year,
        genre: j.genre.clone(),
        cover: j.cover.as_ref().and_then(|p| std::fs::read(p).ok()),
    }
}

impl Library {
    /// Up to `limit` copies at `level` for tracks with an id above `after`.
    fn copies_batch(&self, lvl: Level, after: i64, limit: u32) -> Result<CopyBatch, CoreError> {
        let root = PathBuf::from(self.root());
        let jobs =
            self.with(|c| copies::candidates(c, &root, lvl.kbps(), after, limit.max(1) + 1))?;
        let more = jobs.len() > limit.max(1) as usize;
        let none = AtomicBool::new(false);
        let mut out = CopyBatch {
            made: 0,
            failed: 0,
            more,
            next_after: after,
        };
        let at = |rel: &str| rel.split('/').fold(root.clone(), |p, s| p.join(s));
        for j in jobs.into_iter().take(limit.max(1) as usize) {
            out.next_after = j.id;
            let copy_rel = copies::copy_rel_for(&j.rel_path, j.id);
            let tags = tags_of(&j);
            match encode::to_opus(&at(&j.rel_path), &at(&copy_rel), lvl, &tags, &none) {
                Ok(e) => {
                    self.with(|c| copies::set_copy(c, j.id, &copy_rel, e.bytes, lvl.as_str()))?;
                    out.made += 1;
                }
                Err(_) => out.failed += 1,
            }
        }
        Ok(out)
    }

    fn keep_only(&self, ids: &[i64]) -> Result<Freed, CoreError> {
        let root = PathBuf::from(self.root());
        let mut f = Freed {
            tracks: 0,
            bytes: 0,
        };
        for id in ids {
            if let Some(b) = self.with(|c| copies::keep_copy_only(c, Path::new(&root), *id))? {
                f.tracks += 1;
                f.bytes += b;
            }
        }
        Ok(f)
    }
}

#[uniffi::export]
impl Library {
    pub fn storage_info(&self) -> Result<StorageInfo, CoreError> {
        let s = self.with(|c| copies::storage(c))?;
        Ok(StorageInfo {
            original_bytes: s.original_bytes,
            copy_bytes: s.copy_bytes,
            both: s.both,
            copy_only: s.copy_only,
            original_only: s.original_only,
        })
    }

    /// «Сделать экономные копии»: one batch of up to `limit` tracks at `level` (`economy` |
    /// `max`) after track `after` (0 at first); the app calls again with `next_after` while
    /// `more`, showing progress between batches.
    pub fn make_copies(
        &self,
        level_name: String,
        after: i64,
        limit: u32,
    ) -> Result<CopyBatch, CoreError> {
        self.copies_batch(level(&level_name)?, after, limit)
    }

    /// «Удалить оригиналы, оставить копии»: every track with a copy keeps only the copy.
    pub fn keep_copies_only(&self) -> Result<Freed, CoreError> {
        let ids = self.with(|c| copies::with_copy(c))?;
        self.keep_only(&ids)
    }

    /// «Добавить музыку» in a chosen quality: `None` or `original` — as is; `economy` | `max` —
    /// the new tracks keep only their copy (the original is read once, for recognition and the
    /// copy).
    pub fn import_files_as(
        &self,
        paths: Vec<String>,
        level_name: Option<String>,
    ) -> Result<ImportSummary, CoreError> {
        let lvl = match level_name.as_deref() {
            None | Some("original") => None,
            Some(other) => Some(level(other)?),
        };
        let before = self.with(|c| copies::last_id(c))?;
        let summary = self.import_files(paths)?;
        if let Some(lvl) = lvl {
            let mut after = before;
            loop {
                let b = self.copies_batch(lvl, after, 50)?;
                after = b.next_after;
                if !b.more {
                    break;
                }
            }
            let ids: Vec<i64> = self
                .with(|c| copies::with_copy(c))?
                .into_iter()
                .filter(|id| *id > before)
                .collect();
            self.keep_only(&ids)?;
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::sample;

    #[test]
    fn storage_and_levels_answer_on_an_empty_copy_set() {
        let (lib, dir) = sample();
        let s = lib.storage_info().unwrap();
        assert_eq!((s.original_only, s.copy_only, s.both), (3, 0, 0));
        // The sample's files do not exist: the encoder fails, originals stay.
        let b = lib.make_copies("economy".into(), 0, 2).unwrap();
        assert_eq!((b.made, b.failed, b.more), (0, 2, true));
        let b = lib.make_copies("economy".into(), b.next_after, 2).unwrap();
        assert_eq!(
            (b.made, b.failed, b.more),
            (0, 1, false),
            "a failed track is not offered again"
        );
        assert!(lib.make_copies("lossy".into(), 0, 10).is_err());
        assert_eq!(lib.keep_copies_only().unwrap().tracks, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A real file imported straight into «Экономно»: the library keeps only the Opus copy.
    #[test]
    fn import_in_economy_keeps_only_the_copy() {
        let base = std::env::temp_dir().join(format!("sirtone-ffi-copies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("in");
        std::fs::create_dir_all(&src).unwrap();
        let wav = src.join("Кино - Ёлка.wav");
        let (rate, secs) = (44_100u32, 2u32);
        let frames = rate * secs;
        let mut b = Vec::new();
        let data = frames * 4;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        for i in 0..frames {
            let v = ((i as f32 * 0.0627).sin() * 9_000.0) as i16;
            b.extend_from_slice(&v.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&wav, &b).unwrap();
        let lib =
            crate::library::Library::open(base.join("lib").to_string_lossy().into_owned()).unwrap();
        let s = lib
            .import_files_as(
                vec![wav.to_string_lossy().into_owned()],
                Some("economy".into()),
            )
            .unwrap();
        assert_eq!(s.added, 1, "{s:?}");
        let info = lib.storage_info().unwrap();
        assert_eq!(
            (info.copy_only, info.original_only, info.original_bytes),
            (1, 0, 0)
        );
        assert!(info.copy_bytes > 0 && info.copy_bytes * 4 < b.len() as u64);
        let t = lib.tracks(None, 10).unwrap().tracks;
        let path = lib.track_path(t[0].id).unwrap();
        assert!(path.ends_with(".opus"), "{path}");
        assert!(std::path::Path::new(&path).exists());
        assert!(wav.exists(), "the source outside the library is only read");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Q-34: a copy-only track travels with its mark — the receiving library keeps it as a copy.
    #[test]
    fn a_copy_travels_marked() {
        let base =
            std::env::temp_dir().join(format!("sirtone-ffi-copy-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("in")).unwrap();
        let wav = base.join("in").join("Кино - Звезда.wav");
        let rate = 44_100u32;
        let frames = rate * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + frames * 4).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(frames * 4).to_le_bytes());
        for i in 0..frames {
            let v = ((i as f32 * 0.041).sin() * 8_000.0) as i16;
            b.extend_from_slice(&v.to_le_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&wav, &b).unwrap();
        let phone =
            crate::library::Library::open(base.join("phone").to_string_lossy().into_owned())
                .unwrap();
        phone
            .import_files_as(vec![wav.to_string_lossy().into_owned()], Some("max".into()))
            .unwrap();
        let pkg = base.join("pkg");
        phone
            .export_package(pkg.to_string_lossy().into_owned(), true)
            .unwrap();
        let pc =
            crate::library::Library::open(base.join("pc").to_string_lossy().into_owned()).unwrap();
        let r = pc
            .apply_package(pkg.to_string_lossy().into_owned())
            .unwrap();
        assert_eq!(r.new_tracks, 1, "{r:?}");
        let s = pc.storage_info().unwrap();
        assert_eq!((s.copy_only, s.original_only), (1, 0), "{s:?}");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A copy keeps the cover and the track order: imported again on its own, it has both.
    #[test]
    fn a_copy_carries_cover_and_order() {
        let base =
            std::env::temp_dir().join(format!("sirtone-ffi-copy-tags-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let wav = base.join("src.wav");
        let rate = 44_100u32;
        let frames = rate;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + frames * 2).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(frames * 2).to_le_bytes());
        for i in 0..frames {
            b.extend_from_slice(&(((i as f32 * 0.05).sin() * 7_000.0) as i16).to_le_bytes());
        }
        std::fs::write(&wav, &b).unwrap();
        let mut png = Vec::new();
        image::RgbImage::from_pixel(32, 32, image::Rgb([200, 30, 40]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let tags = Tags {
            title: Some("Звезда по имени Солнце".into()),
            artist: Some("Кино".into()),
            album: Some("Звезда по имени Солнце".into()),
            album_artist: Some("Кино".into()),
            track_no: Some(1),
            year: Some(1989),
            cover: Some(png),
            ..Tags::default()
        };
        let copy = base.join("Кино - Звезда.opus");
        encode::to_opus(&wav, &copy, Level::Economy, &tags, &AtomicBool::new(false)).unwrap();
        let lib =
            crate::library::Library::open(base.join("lib").to_string_lossy().into_owned()).unwrap();
        lib.import_files(vec![copy.to_string_lossy().into_owned()])
            .unwrap();
        let t = lib.tracks(None, 10).unwrap().tracks;
        assert_eq!(t[0].title, "Звезда по имени Солнце");
        assert!(
            lib.track_cover(t[0].id).is_some(),
            "the cover came with the copy"
        );
        let no: Option<i64> = lib
            .with(|c| {
                c.query_row("SELECT track_no FROM track WHERE id = ?1", [t[0].id], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert_eq!(no, Some(1));
        let _ = std::fs::remove_dir_all(&base);
    }
}
