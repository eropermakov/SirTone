//! Disk ↔ DB reconcile (T1.19–T1.20) on a real temp folder with fake probe/tags/identify.
//! Files are small text files: `audio;title=…;artist=…` (anything else is not playable).
//! File ids are `None` here (like FAT/exFAT): moves are found by size + full hash.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite::Connection;
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::ports::*;
use sirtone_library::import::ImportDeps;
use sirtone_library::reconcile::{Outcome, PURGE_AFTER_MS, reconcile};
use sirtone_library::{Medium, open};

static PROBES: AtomicUsize = AtomicUsize::new(0);

struct Probe;
impl MediaProbe for Probe {
    fn probe(&self, path: &Path) -> Result<MediaInfo, AppError> {
        PROBES.fetch_add(1, Ordering::SeqCst);
        let text = fs::read_to_string(path).unwrap_or_default();
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
            ..Tags::default()
        }
    }
}

struct ByTags;
impl Identify for ByTags {
    fn identify(&self, t: &Tags, f: &str, _: &FolderHints, _: &MediaInfo) -> IdentityDecision {
        let ok = t.title.is_some() && t.artist.is_some();
        IdentityDecision {
            title: t.title.clone().unwrap_or_else(|| f.to_string()),
            artists: t.artist.clone().into_iter().collect(),
            album: t.album.clone(),
            album_artist: None,
            track_no: None,
            disc_no: None,
            disc_total: None,
            year: None,
            genre: None,
            confidence: if ok { 0.9 } else { 0.1 },
            unknown: (!ok).then_some(UnknownReason::NoMetadata),
        }
    }
}

struct Ops;
impl FileOps for Ops {
    fn move_durable(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        fs::rename(from, to)
    }
    fn to_recycle_bin(&self, _: &Path) -> std::io::Result<()> {
        Ok(())
    }
}

struct NoCovers;
impl CoverReader for NoCovers {
    fn read(&self, _: &Path, _: Option<&Path>) -> Option<(Vec<u8>, CoverSource)> {
        None
    }
}

struct Fixed;
impl VolumeInfo for Fixed {
    fn is_removable(&self, _: &Path) -> bool {
        false
    }
    fn free_bytes(&self, _: &Path) -> Option<u64> {
        None
    }
}

fn deps() -> ImportDeps {
    ImportDeps {
        probe: Arc::new(Probe),
        tags: Arc::new(TagsFromText),
        identify: Arc::new(ByTags),
        fs: Arc::new(Ops),
        volume: Arc::new(Fixed),
        covers: Arc::new(NoCovers),
    }
}

struct Lib {
    root: PathBuf,
    conn: Connection,
}

impl Lib {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sirtone-rec-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let conn = open(&root.join(".sirtone").join("library.db"), Medium::Fixed).unwrap();
        Self { root, conn }
    }

    fn put(&self, rel: &str, text: &str) {
        let p = self.root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn pass(&mut self, now: i64) -> Outcome {
        reconcile(&mut self.conn, &self.root, &deps(), now, false).unwrap()
    }

    fn id_of(&self, rel: &str) -> Option<i64> {
        self.conn
            .query_row("SELECT id FROM track WHERE rel_path = ?1", [rel], |r| {
                r.get(0)
            })
            .ok()
    }

    fn state(&self, id: i64) -> Option<String> {
        self.conn
            .query_row("SELECT state FROM track WHERE id = ?1", [id], |r| r.get(0))
            .ok()
    }
}

impl Drop for Lib {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn done(o: Outcome) -> sirtone_library::reconcile::Report {
    match o {
        Outcome::Done(r) => r,
        other => panic!("expected Done, got {other:?}"),
    }
}

#[test]
fn explorer_additions_are_adopted_in_place_and_junk_is_remembered() {
    let mut lib = Lib::new("adopt");
    lib.put("Кино/Город.flac", "audio;title=Город;artist=Кино");
    lib.put("Кино/cover.txt", "not audio, not scanned");
    lib.put("junk.mp3", "garbage");
    lib.put(".sirtone/staging/1/x.flac", "audio;title=staged;artist=me");
    let r = done(lib.pass(0));
    assert_eq!((r.adopted, r.ignored), (1, 1));
    assert!(
        lib.id_of("Кино/Город.flac").is_some(),
        "adopted where it is"
    );
    assert!(lib.root.join("Кино/Город.flac").exists(), "nothing moved");
    // Second pass: nothing new, the junk file is not probed again.
    let before = PROBES.load(Ordering::SeqCst);
    let r = done(lib.pass(0));
    assert_eq!(r, Default::default());
    assert_eq!(PROBES.load(Ordering::SeqCst), before);
    // An exact copy of a track is not a second track.
    lib.put("copy/Город.flac", "audio;title=Город;artist=Кино");
    let r = done(lib.pass(0));
    assert_eq!((r.adopted, r.ignored), (0, 1));
}

#[test]
fn rename_keeps_the_row_and_its_journal() {
    let mut lib = Lib::new("move");
    lib.put("a/Песня.flac", "audio;title=Песня;artist=Сплин");
    for i in 0..5 {
        lib.put(
            &format!("other/{i}.flac"),
            &format!("audio;title=T{i};artist=X"),
        );
    }
    done(lib.pass(0));
    let id = lib.id_of("a/Песня.flac").unwrap();
    lib.conn
        .execute(
            "INSERT INTO play_event(ts, track_id, played_ms, duration_ms, end_reason, source,
                                    local_minute, weekday, daypart, season_w)
             VALUES (1, ?1, 1000, 180000, 'finished', 'album', 0, 0, 0, x'ff000000')",
            [id],
        )
        .unwrap();
    fs::create_dir_all(lib.root.join("b")).unwrap();
    fs::rename(
        lib.root.join("a/Песня.flac"),
        lib.root.join("b/Песня (live).flac"),
    )
    .unwrap();
    let r = done(lib.pass(10));
    assert_eq!((r.moved, r.missing_marked, r.adopted), (1, 0, 0));
    assert_eq!(lib.id_of("b/Песня (live).flac"), Some(id));
    let plays: i64 = lib
        .conn
        .query_row(
            "SELECT count(*) FROM play_event WHERE track_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(plays, 1);
}

#[test]
fn a_move_split_over_two_passes_is_still_a_move() {
    let mut lib = Lib::new("split");
    for i in 0..10 {
        lib.put(
            &format!("x/{i}.flac"),
            &format!("audio;title=T{i};artist=X"),
        );
    }
    done(lib.pass(0));
    let id = lib.id_of("x/3.flac").unwrap();
    let content = fs::read(lib.root.join("x/3.flac")).unwrap();
    fs::remove_file(lib.root.join("x/3.flac")).unwrap();
    assert_eq!(done(lib.pass(1)).missing_marked, 1);
    fs::create_dir_all(lib.root.join("y")).unwrap();
    fs::write(lib.root.join("y/3.flac"), content).unwrap();
    let r = done(lib.pass(2));
    assert_eq!((r.moved, r.adopted), (1, 0));
    assert_eq!(lib.id_of("y/3.flac"), Some(id));
    assert_eq!(lib.state(id).as_deref(), Some("ok"));
}

#[test]
fn missing_comes_back_and_is_purged_after_a_day() {
    let mut lib = Lib::new("missing");
    for i in 0..10 {
        lib.put(
            &format!("m/{i}.flac"),
            &format!("audio;title=T{i};artist=X"),
        );
    }
    done(lib.pass(0));
    let id = lib.id_of("m/0.flac").unwrap();
    let saved = fs::read(lib.root.join("m/0.flac")).unwrap();
    fs::remove_file(lib.root.join("m/0.flac")).unwrap();
    let t0 = 1_000_000;
    assert_eq!(done(lib.pass(t0)).missing_marked, 1);
    assert_eq!(lib.state(id).as_deref(), Some("missing"));
    // Back: ok again.
    fs::write(lib.root.join("m/0.flac"), &saved).unwrap();
    assert_eq!(done(lib.pass(t0 + 1)).returned, 1);
    assert_eq!(lib.state(id).as_deref(), Some("ok"));
    // Gone again: kept for 24 h, then purged.
    fs::remove_file(lib.root.join("m/0.flac")).unwrap();
    done(lib.pass(t0 + 2));
    assert_eq!(done(lib.pass(t0 + 2 + PURGE_AFTER_MS - 1)).purged, 0);
    assert_eq!(done(lib.pass(t0 + 2 + PURGE_AFTER_MS)).purged, 1);
    assert_eq!(lib.state(id), None);
}

#[test]
fn mass_disappearance_is_a_question_and_changes_nothing() {
    let mut lib = Lib::new("mass");
    for i in 0..10 {
        lib.put(
            &format!("album/{i}.flac"),
            &format!("audio;title=T{i};artist=X"),
        );
    }
    done(lib.pass(0));
    for i in 0..5 {
        fs::remove_file(lib.root.join(format!("album/{i}.flac"))).unwrap();
    }
    match lib.pass(1) {
        Outcome::Question { gone, .. } => assert_eq!(gone, 5),
        other => panic!("{other:?}"),
    }
    let missing: i64 = lib
        .conn
        .query_row(
            "SELECT count(*) FROM track WHERE state = 'missing'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(missing, 0, "nothing marked while the question is open");
    // The user confirms: now they are marked.
    let r = reconcile(&mut lib.conn, &lib.root, &deps(), 2, true).unwrap();
    assert_eq!(done(r).missing_marked, 5);
}

#[test]
fn unreadable_root_is_offline_and_touches_nothing() {
    let mut lib = Lib::new("offline");
    for i in 0..3 {
        lib.put(
            &format!("o/{i}.flac"),
            &format!("audio;title=T{i};artist=X"),
        );
    }
    done(lib.pass(0));
    // The DB stays open; the root it belongs to is not there (renamed, stick pulled).
    let missing_root = lib.root.join("not-here");
    assert_eq!(
        reconcile(&mut lib.conn, &missing_root, &deps(), 5, false).unwrap(),
        Outcome::Offline
    );
    assert!(!missing_root.exists(), "an offline root is never recreated");
    let ok: i64 = lib
        .conn
        .query_row("SELECT count(*) FROM track WHERE state = 'ok'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(ok, 3);
}
