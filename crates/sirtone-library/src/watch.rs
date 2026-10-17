//! File watcher over the library root (docs/05 §7, T1.19): `notify` (ReadDirectoryChangesW) with
//! a 500 ms debounce. It only says «something changed»; the reconcile pass on the import worker
//! decides what. Our own service folder and `.part` files are ignored.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::import::SERVICE_DIR;

pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// Keeps the watch alive; dropping it stops watching.
pub struct Watch {
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

/// Whether a changed path matters for the library.
fn relevant(root: &Path, p: &Path) -> bool {
    let rel = p.strip_prefix(root).unwrap_or(p);
    let in_service = rel
        .components()
        .next()
        .is_some_and(|c| c.as_os_str() == SERVICE_DIR);
    let part = p
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("part"));
    !in_service && !part
}

/// Calls `on_change` (from the watcher's thread) after relevant changes settle, and on errors
/// or overflow (a full rescan is then needed — the reconcile pass is always full).
pub fn watch(
    root: &Path,
    on_change: impl Fn() + Send + 'static,
) -> Result<Watch, notify_debouncer_full::notify::Error> {
    let r: PathBuf = root.to_path_buf();
    let mut debouncer = new_debouncer(DEBOUNCE, None, move |res: DebounceEventResult| {
        let fire = match res {
            Ok(events) => events
                .iter()
                .any(|e| e.need_rescan() || e.paths.iter().any(|p| relevant(&r, p))),
            Err(_) => true,
        };
        if fire {
            on_change();
        }
    })?;
    debouncer.watch(root, RecursiveMode::Recursive)?;
    Ok(Watch {
        _debouncer: debouncer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn service_folder_and_part_files_are_ignored() {
        let root = Path::new(r"D:\Music");
        assert!(relevant(root, Path::new(r"D:\Music\Кино\Город.flac")));
        assert!(!relevant(
            root,
            Path::new(r"D:\Music\.sirtone\library.db-wal")
        ));
        assert!(!relevant(root, Path::new(r"D:\Music\Кино\Город.flac.part")));
    }

    #[test]
    fn a_new_file_fires_within_two_seconds() {
        let root = std::env::temp_dir().join(format!("sirtone-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(SERVICE_DIR)).unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let _w = watch(&root, move || {
            h.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(100));
        std::fs::write(root.join(SERVICE_DIR).join("x.db"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(900));
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "service folder changes are ignored"
        );
        let t = std::time::Instant::now();
        std::fs::write(root.join("new.flac"), b"x").unwrap();
        while hits.load(Ordering::SeqCst) == 0 && t.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(hits.load(Ordering::SeqCst) > 0, "not seen within 2 s");
        drop(_w);
        let _ = std::fs::remove_dir_all(&root);
    }
}
