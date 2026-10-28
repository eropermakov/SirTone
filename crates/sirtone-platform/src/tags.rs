//! WMA/ASF tags through the Windows Property System (T1.15, docs/04 §2: lofty does not read ASF).
//! Other formats go to the wrapped reader unchanged.

use std::path::Path;
use std::sync::Arc;

use sirtone_core::ports::{TagReader, Tags};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Storage::EnhancedStorage::{
    PKEY_Media_Year, PKEY_Music_AlbumArtist, PKEY_Music_AlbumTitle, PKEY_Music_Artist,
    PKEY_Music_Genre, PKEY_Music_PartOfSet, PKEY_Music_TrackNumber, PKEY_Title,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::PropertiesSystem::{
    GPS_DEFAULT, IPropertyStore, SHGetPropertyStoreFromParsingName,
};
use windows::core::{BSTR, HSTRING};

/// `inner` for everything except WMA/ASF, the shell's property handler for those.
pub struct WinTags {
    inner: Arc<dyn TagReader>,
}

impl WinTags {
    pub fn new(inner: Arc<dyn TagReader>) -> Self {
        Self { inner }
    }
}

fn is_asf(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "wma" | "asf"))
}

impl TagReader for WinTags {
    fn read(&self, path: &Path) -> Tags {
        if is_asf(path) {
            read_shell(path).unwrap_or_default()
        } else {
            self.inner.read(path)
        }
    }
}

fn text(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
    let v = unsafe { store.GetValue(key) }.ok()?;
    // Multi-value fields (several artists) come back joined with «; ». ASF files often carry the
    // same artist twice (Author + WM/…): keep each value once.
    let s = BSTR::try_from(&v).ok()?.to_string();
    let mut parts: Vec<&str> = Vec::new();
    for p in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        if !parts.contains(&p) {
            parts.push(p);
        }
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

fn number(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<u32> {
    let v = unsafe { store.GetValue(key) }.ok()?;
    u32::try_from(&v).ok().filter(|n| *n > 0)
}

/// «n/m» or «n» (WM/PartOfSet is a string).
fn part_of_set(s: &str) -> (Option<u32>, Option<u32>) {
    let mut it = s
        .split('/')
        .map(|p| p.trim().parse::<u32>().ok().filter(|n| *n > 0));
    (it.next().flatten(), it.next().flatten())
}

fn read_shell(path: &Path) -> Option<Tags> {
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    // The shell parser wants a plain absolute path with backslashes (no `/`, no `\\?\`).
    let abs = std::path::absolute(path).ok()?;
    let plain = abs.to_string_lossy().replace('/', "\\");
    let plain = plain.strip_prefix(r"\\?\").unwrap_or(&plain);
    let store: IPropertyStore =
        unsafe { SHGetPropertyStoreFromParsingName(&HSTRING::from(plain), None, GPS_DEFAULT) }
            .ok()?;
    let (disc_no, disc_total) = text(&store, &PKEY_Music_PartOfSet)
        .map(|s| part_of_set(&s))
        .unwrap_or_default();
    Some(Tags {
        title: text(&store, &PKEY_Title),
        artist: text(&store, &PKEY_Music_Artist),
        album: text(&store, &PKEY_Music_AlbumTitle),
        album_artist: text(&store, &PKEY_Music_AlbumArtist),
        track_no: number(&store, &PKEY_Music_TrackNumber),
        disc_no,
        disc_total,
        year: number(&store, &PKEY_Media_Year).and_then(|y| i32::try_from(y).ok()),
        genre: text(&store, &PKEY_Music_Genre),
        mbid_recording: None,
        mbid_release: None,
        mbid_group: None,
        loudness: Default::default(),
        lang: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Never;
    impl TagReader for Never {
        fn read(&self, _: &Path) -> Tags {
            panic!("inner reader used for WMA")
        }
    }

    #[test]
    fn wma_tags_come_from_the_property_system() {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("fixtures")
            .join("generated")
            .join("tags")
            .join("wma-tagged.wma");
        assert!(p.is_absolute());
        if !p.exists() {
            return; // corpus not generated
        }
        let t = WinTags::new(Arc::new(Never)).read(&p);
        assert_eq!(t.title.as_deref(), Some("Город"));
        assert_eq!(t.artist.as_deref(), Some("Кино"));
        assert_eq!(t.album.as_deref(), Some("Группа крови"));
        assert_eq!(t.album_artist.as_deref(), Some("Кино"));
        assert_eq!(t.track_no, Some(3));
        assert_eq!((t.disc_no, t.disc_total), (Some(1), Some(2)));
        assert_eq!(t.year, Some(1988));
        assert_eq!(t.genre.as_deref(), Some("Rock"));
        // Paths with forward slashes (from tools and dialogs) parse too.
        let slashed = PathBuf::from(p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/"));
        let t = WinTags::new(Arc::new(Never)).read(&slashed);
        assert_eq!(t.title.as_deref(), Some("Город"));
    }

    #[test]
    fn part_of_set_forms() {
        assert_eq!(part_of_set("2/3"), (Some(2), Some(3)));
        assert_eq!(part_of_set("1"), (Some(1), None));
        assert_eq!(part_of_set("x"), (None, None));
    }
}
