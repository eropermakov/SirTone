//! Offline cover art (FR-REC-009, docs/01b F-12): embedded picture (front cover preferred) →
//! `cover`/`folder`/`front` image in the source folder.

use std::path::Path;

use lofty::picture::{MimeType, PictureType};
use lofty::prelude::*;
use sirtone_core::ports::{CoverReader, CoverSource};

/// Larger embedded pictures are ignored (docs/08 §6: limits on cover sizes from untrusted files).
const MAX_COVER_BYTES: usize = 20 * 1024 * 1024;
const FOLDER_NAMES: &[&str] = &["cover", "folder", "front", "albumart", "album"];

pub struct LoftyCovers;

fn is_image(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF]) || bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

fn embedded(file: &Path) -> Option<Vec<u8>> {
    let tagged = lofty::read_from_path(file).ok()?;
    let mut best: Option<Vec<u8>> = None;
    for tag in tagged.tags() {
        for pic in tag.pictures() {
            if !matches!(pic.mime_type(), Some(MimeType::Jpeg | MimeType::Png) | None) {
                continue;
            }
            let data = pic.data();
            if data.len() > MAX_COVER_BYTES || !is_image(data) {
                continue;
            }
            if pic.pic_type() == PictureType::CoverFront {
                return Some(data.to_vec());
            }
            best.get_or_insert_with(|| data.to_vec());
        }
    }
    best
}

fn folder(dir: &Path) -> Option<Vec<u8>> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut candidates: Vec<(usize, std::path::PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let ext = p.extension()?.to_string_lossy().to_ascii_lowercase();
            if !matches!(ext.as_str(), "jpg" | "jpeg" | "png") {
                return None;
            }
            let stem = p.file_stem()?.to_string_lossy().to_ascii_lowercase();
            let rank = FOLDER_NAMES.iter().position(|n| stem == *n)?;
            Some((rank, p))
        })
        .collect();
    candidates.sort();
    candidates.into_iter().find_map(|(_, p)| {
        let meta = std::fs::metadata(&p).ok()?;
        if meta.len() as usize > MAX_COVER_BYTES {
            return None;
        }
        let bytes = std::fs::read(&p).ok()?;
        is_image(&bytes).then_some(bytes)
    })
}

impl CoverReader for LoftyCovers {
    fn read(&self, file: &Path, source_dir: Option<&Path>) -> Option<(Vec<u8>, CoverSource)> {
        if let Some(b) = embedded(file) {
            return Some((b, CoverSource::Embedded));
        }
        source_dir
            .and_then(folder)
            .map(|b| (b, CoverSource::Folder))
    }
}
