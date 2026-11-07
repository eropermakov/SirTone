//! `manifest.json`: where the package comes from, what parts it carries and every music file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{FORMAT, MANIFEST_FILE, MUSIC_DIR, TransferError, safe_rel_path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Ios,
    Android,
}

/// The library the package was made from. `library_uuid` is the library's own id (`meta` table),
/// so keys made from it are unique across all devices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub platform: Platform,
    pub library_uuid: String,
    pub app_version: String,
}

/// The four checkboxes of the hub (MR §11): «Музыка», «Плейлисты», «Вкусы и волна», «Настройки и тема».
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parts {
    pub music: bool,
    pub playlists: bool,
    pub taste: bool,
    pub settings: bool,
}

/// One music file in the package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackEntry {
    /// `<library_uuid>:<track id>` of the source library — what events refer to.
    pub key: String,
    /// Package-relative path under `music/`, `/`-separated.
    pub file: String,
    pub size: u64,
    pub duration_ms: u32,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    /// Chromaprint items (raw u32), when the source analysed the track. On disk: Chromaprint's
    /// compressed form in URL-safe base64, as `fpcalc` prints it (~1 KB instead of ~17 KB).
    #[serde(default, skip_serializing_if = "Option::is_none", with = "fp_codec")]
    pub fingerprint: Option<Vec<u32>>,
    /// BLAKE3 of the whole file: the exact same file is recognised before any sound match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    /// The file is an economy copy at this level (`economy` | `max`, D-146): the receiving side
    /// marks the track so, and an original found there later replaces it (Q-34).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy: Option<String>,
}

mod fp_codec {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    use sirtone_core::fingerprint::{base64url, compress, decompress};

    /// Chromaprint's own algorithm id (`fpcalc` default).
    const ALGORITHM: u8 = 1;

    pub fn serialize<S: Serializer>(fp: &Option<Vec<u32>>, s: S) -> Result<S::Ok, S::Error> {
        match fp {
            Some(fp) => s.serialize_str(&base64url(&compress(fp, ALGORITHM))),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u32>>, D::Error> {
        let Some(text) = Option::<String>::deserialize(d)? else {
            return Ok(None);
        };
        let bytes = unbase64url(&text).ok_or_else(|| D::Error::custom("fingerprint: base64"))?;
        decompress(&bytes)
            .map(Some)
            .ok_or_else(|| D::Error::custom("fingerprint: data"))
    }

    fn unbase64url(s: &str) -> Option<Vec<u8>> {
        let val = |c: u8| match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        };
        let mut out = Vec::with_capacity(s.len() * 3 / 4);
        let (mut acc, mut bits) = (0u32, 0u32);
        for c in s.bytes() {
            acc = (acc << 6) | u32::from(val(c)?);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
                acc &= (1 << bits) - 1;
            }
        }
        Some(out)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// Unique per package: resume continues only the same package.
    pub package_id: String,
    pub created_ms: i64,
    pub source: Source,
    pub parts: Parts,
    #[serde(default)]
    pub tracks: Vec<TrackEntry>,
    /// Playlists as `{ name, track keys }` — kept as JSON so the library owns their schema.
    #[serde(default)]
    pub playlists: Vec<PlaylistEntry>,
    /// Settings and theme as the app's settings JSON (only with `parts.settings`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub name: String,
    pub tracks: Vec<String>,
}

impl Manifest {
    pub fn new(package_id: String, created_ms: i64, source: Source, parts: Parts) -> Self {
        Self {
            format: FORMAT,
            package_id,
            created_ms,
            source,
            parts,
            tracks: Vec::new(),
            playlists: Vec::new(),
            settings: None,
        }
    }

    /// Size of everything to copy — shown before the transfer starts («Не хватает 2,4 ГБ»).
    pub fn total_bytes(&self) -> u64 {
        self.tracks.iter().map(|t| t.size).sum()
    }

    /// Every path stays inside `music/` and is portable; formats from the future are refused.
    pub fn validate(&self) -> Result<(), TransferError> {
        if self.format > FORMAT {
            return Err(TransferError::NewerFormat(self.format));
        }
        for t in &self.tracks {
            let inside = t
                .file
                .strip_prefix(MUSIC_DIR)
                .is_some_and(|rest| rest.starts_with('/'));
            if !inside || !safe_rel_path(&t.file) {
                return Err(TransferError::UnsafePath(t.file.clone()));
            }
        }
        Ok(())
    }

    pub fn write(&self, dir: &Path) -> Result<(), TransferError> {
        std::fs::create_dir_all(dir)?;
        // Write then rename: a cut cable never leaves half a manifest.
        let tmp = dir.join(format!("{MANIFEST_FILE}.part"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, dir.join(MANIFEST_FILE))?;
        Ok(())
    }

    pub fn read(dir: &Path) -> Result<Self, TransferError> {
        let m: Self = serde_json::from_slice(&std::fs::read(dir.join(MANIFEST_FILE))?)?;
        m.validate()?;
        Ok(m)
    }

    /// Resume (MR §11 «Докачка»): the files still to copy — absent at the destination or of
    /// another size. `present` answers the size of a package path at the destination.
    pub fn pending(&self, present: impl Fn(&str) -> Option<u64>) -> Vec<&TrackEntry> {
        self.tracks
            .iter()
            .filter(|t| present(&t.file) != Some(t.size))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn sample() -> Manifest {
        let mut m = Manifest::new(
            "p1".into(),
            1_762_300_000_000,
            Source {
                platform: Platform::Windows,
                library_uuid: "lib-pc".into(),
                app_version: "0.1.38".into(),
            },
            Parts {
                music: true,
                playlists: true,
                taste: true,
                settings: false,
            },
        );
        for (i, size) in [(1, 10u64), (2, 20)] {
            m.tracks.push(TrackEntry {
                key: format!("lib-pc:{i}"),
                file: format!("music/Артист/{i:02} Трек.flac"),
                size,
                duration_ms: 180_000,
                artist: Some("Артист".into()),
                title: Some(format!("Трек {i}")),
                album: None,
                fingerprint: None,
                hash: None,
                copy: None,
            });
        }
        m
    }

    #[test]
    fn round_trip_and_resume() {
        let dir = std::env::temp_dir().join(format!("sirtone-transfer-{}", std::process::id()));
        let mut m = sample();
        m.tracks[0].fingerprint =
            Some((0..900u32).map(|i| i.wrapping_mul(2_654_435_761)).collect());
        m.write(&dir).unwrap();
        let back = Manifest::read(&dir).unwrap();
        assert_eq!(back, m);
        let text = std::fs::read_to_string(dir.join(crate::MANIFEST_FILE)).unwrap();
        assert!(
            text.len() < 10_000,
            "compact fingerprints: {} bytes",
            text.len()
        );
        assert_eq!(back.total_bytes(), 30);
        // The first file arrived whole, the second was cut at 7 bytes.
        let pending = back.pending(|p| match p {
            "music/Артист/01 Трек.flac" => Some(10),
            _ => Some(7),
        });
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].key, "lib-pc:2");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_escapes_and_newer_formats() {
        let mut m = sample();
        m.tracks[0].file = "music/../../Windows/x.dll".into();
        assert_eq!(m.validate().unwrap_err().code(), "transfer.unsafe_path");
        let mut m = sample();
        m.tracks[0].file = "musicx/a.flac".into();
        assert!(m.validate().is_err());
        let mut m = sample();
        m.format = FORMAT + 1;
        assert_eq!(m.validate().unwrap_err().code(), "transfer.newer_format");
    }
}
