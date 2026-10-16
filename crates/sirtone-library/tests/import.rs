//! Import pipeline with fake probe/tags/identify (real ones come from audio/recognize in the app).
//! Test files are small text files: `audio;title=…;artist=…;album=…;track=…` or anything else.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::ports::*;
use sirtone_library::import::{self, ImportDeps, ImportEvent};
use sirtone_library::{collection, open_in_memory};

struct Probe;
impl MediaProbe for Probe {
    fn probe(&self, path: &Path) -> Result<MediaInfo, AppError> {
        let text = fs::read_to_string(path).unwrap_or_default();
        if text.starts_with("corrupt") {
            return Err(AppError::new(ErrorCode::DecodeFailed));
        }
        if !text.starts_with("audio") {
            return Err(AppError::new(ErrorCode::UnsupportedFormat));
        }
        Ok(MediaInfo {
            container: "flac".into(),
            codec: "flac".into(),
            lossless: true,
            sample_rate: Some(44100),
            channels: Some(2),
            duration_ms: 180_000,
        })
    }
}

struct TagsFromText;
impl TagReader for TagsFromText {
    fn read(&self, path: &Path) -> Tags {
        let text = fs::read_to_string(path).unwrap_or_default();
        let get = |k: &str| {
            text.split(';')
                .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
                .map(str::to_owned)
        };
        Tags {
            title: get("title"),
            artist: get("artist"),
            album: get("album"),
            track_no: get("track").and_then(|t| t.parse().ok()),
            ..Tags::default()
        }
    }
}

/// Stand-in for recognize (T1.17): confident with title + artist, else «Неизвестные».
struct ByTags;
impl Identify for ByTags {
    fn identify(
        &self,
        t: &Tags,
        file_name: &str,
        _: Option<&str>,
        _: &MediaInfo,
    ) -> IdentityDecision {
        let confident = t.title.is_some() && t.artist.is_some();
        IdentityDecision {
            title: t.title.clone().unwrap_or_else(|| file_name.to_string()),
            artists: t.artist.clone().into_iter().collect(),
            album: t.album.clone(),
            album_artist: None,
            track_no: t.track_no,
            disc_no: None,
            disc_total: None,
            year: None,
            genre: None,
            confidence: if confident { 0.9 } else { 0.1 },
            unknown: (!confident).then_some(UnknownReason::NoMetadata),
        }
    }
}

/// Plain rename; optionally "kills the process" (panics, unwinding out of the whole run) right
/// after the n-th move succeeded — the file is moved, nothing after it ran.
#[derive(Default)]
struct Ops {
    moves: AtomicUsize,
    fail_after_move: Option<usize>,
}
impl FileOps for Ops {
    fn move_durable(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        if to.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "exists",
            ));
        }
        fs::rename(from, to)?;
        let n = self.moves.fetch_add(1, Ordering::SeqCst) + 1;
        if self.fail_after_move == Some(n) {
            panic!("simulated kill after move {n}");
        }
        Ok(())
    }
    fn to_recycle_bin(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

struct NoVolume;
impl VolumeInfo for NoVolume {
    fn is_removable(&self, _: &Path) -> bool {
        false
    }
    fn free_bytes(&self, _: &Path) -> Option<u64> {
        None
    }
}

fn deps(ops: Ops) -> ImportDeps {
    ImportDeps {
        probe: Arc::new(Probe),
        tags: Arc::new(TagsFromText),
        identify: Arc::new(ByTags),
        fs: Arc::new(ops),
        volume: Arc::new(NoVolume),
    }
}

struct Sandbox {
    dir: PathBuf,
}
impl Sandbox {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("sirtone-import-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src/Би-2/Иномарки")).unwrap();
        fs::create_dir_all(dir.join("lib")).unwrap();
        let w = |rel: &str, text: &str| fs::write(dir.join("src").join(rel), text).unwrap();
        w(
            "Би-2/Иномарки/01.flac",
            "audio;title=Варвара;artist=Би-2;album=Иномарки;track=1",
        );
        w(
            "Би-2/Иномарки/02.flac",
            "audio;title=Полковнику;artist=Би-2;album=Иномарки;track=2",
        );
        w("Би-2/single.mp3", "audio;title=Мой рок-н-ролл;artist=Би-2");
        w("Track 01.mp3", "audio");
        w(
            "copy of 01.flac",
            "audio;title=Варвара;artist=Би-2;album=Иномарки;track=1",
        );
        w("notes.txt", "hello");
        w("broken.flac", "corrupt");
        w("list.m3u", "#EXTM3U");
        Sandbox { dir }
    }
    fn src(&self) -> PathBuf {
        self.dir.join("src")
    }
    fn lib(&self) -> PathBuf {
        self.dir.join("lib")
    }
    fn source_hashes(&self) -> Vec<(PathBuf, String)> {
        let mut out = Vec::new();
        for e in walk(&self.src()) {
            out.push((e.clone(), import::hash_file(&e).unwrap()));
        }
        out.sort();
        out
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
fn walk(p: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    for e in fs::read_dir(p).unwrap().flatten() {
        if e.path().is_dir() {
            v.extend(walk(&e.path()));
        } else {
            v.push(e.path());
        }
    }
    v
}

#[test]
fn imports_places_reports_and_never_touches_sources() {
    let sb = Sandbox::new("full");
    let before = sb.source_hashes();
    let mut db = open_in_memory().unwrap();
    let batch = import::start_batch(&mut db, &[sb.src()], "drop").unwrap();
    let mut events = Vec::new();
    let report = import::run_batch(
        &mut db,
        &sb.lib(),
        batch,
        &deps(Ops::default()),
        &AtomicBool::new(false),
        &mut |e| events.push(e),
    )
    .unwrap();

    assert_eq!(report.added, 4, "{report:?}");
    assert_eq!(report.unknown, 1);
    assert_eq!(report.duplicates.len(), 1, "exact copy");
    let mut reasons: Vec<_> = report.rejected.iter().map(|(_, r)| r.as_str()).collect();
    reasons.sort();
    assert_eq!(reasons, ["corrupt", "not_audio", "playlist"]);
    assert!(report.can_eject);
    assert!(events.contains(&ImportEvent::SourcesReleased));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ImportEvent::TrackAdded { .. }))
            .count(),
        4
    );

    let lib = sb.lib();
    assert!(lib.join("Би-2/Иномарки/01 - Варвара.flac").exists());
    assert!(lib.join("Би-2/Иномарки/02 - Полковнику.flac").exists());
    assert!(lib.join("Би-2/Singles/Мой рок-н-ролл.mp3").exists());
    assert!(lib.join("_Unknown/Track 01.mp3").exists());
    assert!(
        !lib.join(".sirtone/staging")
            .join(batch.to_string())
            .exists(),
        "staging cleaned"
    );
    assert_eq!(sb.source_hashes(), before, "sources unchanged");

    let n = collection::counts(&db).unwrap();
    assert_eq!((n.tracks, n.artists, n.albums, n.unknown), (4, 1, 1, 1));
    assert!(import::unfinished_batches(&db).unwrap().is_empty());

    // Same files again → all exact duplicates, nothing new on disk.
    let again = import::start_batch(&mut db, &[sb.src()], "drop").unwrap();
    let r2 = import::run_batch(
        &mut db,
        &lib,
        again,
        &deps(Ops::default()),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(r2.added, 0);
    assert_eq!(r2.duplicates.len(), 5);
    assert_eq!(collection::counts(&db).unwrap().tracks, 4);
}

#[test]
fn cancel_before_start_leaves_nothing_behind() {
    let sb = Sandbox::new("cancel");
    let mut db = open_in_memory().unwrap();
    let batch = import::start_batch(&mut db, &[sb.src()], "drop").unwrap();
    let r = import::run_batch(
        &mut db,
        &sb.lib(),
        batch,
        &deps(Ops::default()),
        &AtomicBool::new(true),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(r.added, 0);
    assert_eq!(r.cancelled, 8);
    assert!(!r.can_eject);
    assert!(
        !sb.lib()
            .join(".sirtone/staging")
            .join(batch.to_string())
            .exists()
    );
    assert!(import::unfinished_batches(&db).unwrap().is_empty());
}

/// A crash between «file moved into place» and «transaction committed», at every possible
/// placement, followed by recovery and resume: no loss, no duplicates, no stray files.
#[test]
fn crash_after_any_move_recovers_and_resumes() {
    for crash_at in 1.. {
        let sb = Sandbox::new(&format!("crash{crash_at}"));
        let before = sb.source_hashes();
        let mut db = open_in_memory().unwrap();
        let batch = import::start_batch(&mut db, &[sb.src()], "drop").unwrap();
        let crashing = deps(Ops {
            fail_after_move: Some(crash_at),
            ..Ops::default()
        });
        let killed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            import::run_batch(
                &mut db,
                &sb.lib(),
                batch,
                &crashing,
                &AtomicBool::new(false),
                &mut |_| {},
            )
        }));
        if killed.is_ok() {
            // Past the last move of the batch: every crash point has been covered.
            assert!(crash_at > 8, "only {} moves in the batch", crash_at - 1);
            break;
        }
        // "Restart": recover, then resume every unfinished batch with healthy deps.
        let ops = Ops::default();
        import::recover(&mut db, &sb.lib(), &ops).unwrap();
        for b in import::unfinished_batches(&db).unwrap() {
            import::run_batch(
                &mut db,
                &sb.lib(),
                b,
                &deps(Ops::default()),
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .unwrap();
        }
        let n = collection::counts(&db).unwrap();
        assert_eq!(n.tracks, 4, "crash at move {crash_at}");
        let planned: i64 = db
            .query_row(
                "SELECT count(*) FROM fs_journal WHERE state = 'planned'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(planned, 0, "crash at move {crash_at}");
        // Every library file is indexed; nothing left in staging; sources intact.
        let files: Vec<_> = walk(&sb.lib())
            .into_iter()
            .filter(|p| !p.to_string_lossy().contains(".sirtone"))
            .collect();
        assert_eq!(files.len(), 4, "crash at move {crash_at}: {files:?}");
        let staged = sb.lib().join(".sirtone/staging");
        assert!(
            !staged.exists() || walk(&staged).is_empty(),
            "crash at move {crash_at}"
        );
        assert_eq!(sb.source_hashes(), before);
    }
}
