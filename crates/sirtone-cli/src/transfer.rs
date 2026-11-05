//! `sirtone-cli transfer export <library_root> <out_dir> [--no-music]` — the transfer package of
//! a library (D-140, M6.2; check П7 uses it): `manifest.json`, `events.jsonl` and, unless
//! `--no-music`, the music under `music/`. Run again after a cut cable: only the missing or
//! short files are copied (resume by manifest). Audio is copied byte for byte.
//! Run it on a COPY of a library (the database is opened writable: migrations go forward).

use std::path::Path;
use std::process::ExitCode;

use sirtone_library::transfer::{self, JournalRow};
use sirtone_transfer::{
    EVENTS_FILE, Event, EventKind, MUSIC_DIR, Manifest, Parts, Platform, Source, TrackEntry,
    manifest::PlaylistEntry,
};

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

pub fn export(root: &str, out: &str, music: bool) -> ExitCode {
    match run(Path::new(root), Path::new(out), music) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("transfer export: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(root: &Path, out: &Path, music: bool) -> Result<(), String> {
    let conn = crate::library::open_existing(root)?;
    let s = |e: sirtone_library::LibraryError| e.to_string();
    let uid = transfer::library_uid(&conn).map_err(s)?;
    let key = |track: i64| format!("{uid}:{track}");

    // Resume: the same package keeps its id and manifest.
    let mut manifest = match Manifest::read(out) {
        Ok(m) if m.source.library_uuid == uid => m,
        _ => {
            let mut m = Manifest::new(
                format!("{uid}:{}", now_ms()),
                now_ms(),
                Source {
                    platform: Platform::Windows,
                    library_uuid: uid.clone(),
                    app_version: env!("CARGO_PKG_VERSION").to_owned(),
                },
                Parts {
                    music,
                    playlists: true,
                    taste: true,
                    settings: false,
                },
            );
            for t in transfer::tracks(&conn).map_err(s)? {
                m.tracks.push(TrackEntry {
                    key: key(t.id),
                    file: format!("{MUSIC_DIR}/{}", t.rel_path),
                    size: t.size,
                    duration_ms: t.duration_ms,
                    artist: t.artist,
                    title: Some(t.title),
                    album: t.album,
                    fingerprint: t
                        .fingerprint
                        .as_deref()
                        .and_then(sirtone_core::fingerprint::decompress),
                });
            }
            for p in sirtone_library::playlists::list(&conn).map_err(s)? {
                if p.smart {
                    continue; // rules, not lists: the phone rebuilds smart playlists itself
                }
                let tracks = sirtone_library::playlists::tracks(&conn, p.id).map_err(s)?;
                m.playlists.push(PlaylistEntry {
                    name: p.name,
                    tracks: tracks.iter().map(|t| key(t.track.id)).collect(),
                });
            }
            m.validate().map_err(|e| e.to_string())?;
            m.write(out).map_err(|e| e.to_string())?;
            m
        }
    };
    if !music {
        manifest.parts.music = false;
    }

    let events: Vec<Event> = transfer::journal(&conn)
        .map_err(s)?
        .into_iter()
        .map(|r| match r {
            JournalRow::Play {
                id,
                ts,
                track_id,
                played_ms,
                duration_ms,
                end_reason,
                source,
            } => Event {
                key: format!("{uid}:play:{id}"),
                track_key: key(track_id),
                ts,
                kind: EventKind::Play {
                    played_ms: played_ms.clamp(0, i64::from(u32::MAX)) as u32,
                    duration_ms: duration_ms.clamp(0, i64::from(u32::MAX)) as u32,
                    end_reason,
                    source,
                },
            },
            JournalRow::Mark {
                id,
                ts,
                track_id,
                kind,
            } => Event {
                key: format!("{uid}:mark:{id}"),
                track_key: key(track_id),
                ts,
                kind: EventKind::Mark { kind },
            },
        })
        .collect();
    let mut buf = Vec::new();
    sirtone_transfer::events::write_jsonl(&mut buf, &events).map_err(|e| e.to_string())?;
    std::fs::write(out.join(EVENTS_FILE), buf).map_err(|e| e.to_string())?;

    let (mut copied, mut bytes) = (0u32, 0u64);
    if manifest.parts.music {
        let size_at = |p: &str| {
            std::fs::metadata(out.join(p.replace('/', std::path::MAIN_SEPARATOR_STR)))
                .ok()
                .map(|m| m.len())
        };
        let pending: Vec<TrackEntry> = manifest.pending(size_at).into_iter().cloned().collect();
        for t in &pending {
            let rel = t.file.strip_prefix("music/").unwrap_or(&t.file);
            let src = rel
                .split('/')
                .fold(root.to_path_buf(), |p, part| p.join(part));
            let dst = t
                .file
                .split('/')
                .fold(out.to_path_buf(), |p, part| p.join(part));
            if let Some(dir) = dst.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            // `.part` then rename: a cut cable never leaves a short file under the real name.
            let part = dst.with_extension("sirtone-part");
            match std::fs::copy(&src, &part).and_then(|_| std::fs::rename(&part, &dst)) {
                Ok(()) => {
                    copied += 1;
                    bytes += t.size;
                }
                Err(e) => eprintln!("{}: {e}", t.file),
            }
        }
    }
    println!(
        "package {}: {} tracks ({} MB), {} playlists, {} events; copied now {copied} files ({} MB)",
        manifest.package_id,
        manifest.tracks.len(),
        manifest.total_bytes() / 1_000_000,
        manifest.playlists.len(),
        events.len(),
        bytes / 1_000_000,
    );
    Ok(())
}
