//! `sirtone-cli transfer export <library_root> <out_dir> [--no-music]` — the transfer package of
//! a library (D-140, M6.2; check П7 uses it): `manifest.json`, `events.jsonl` and, unless
//! `--no-music`, the music under `music/`. Run again after a cut cable: only the missing or
//! short files are copied (resume by manifest). Audio is copied byte for byte.
//! `sirtone-cli transfer import <library_root> <package_dir>` — the other way (phone → PC): new
//! music goes through the ordinary import (recognition, layout, duplicates by content), then
//! plays, marks and playlists are applied once each (D-140).
//! Run both on a COPY of a library (the database is opened writable: migrations go forward).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::ExitCode;

use sirtone_library::transfer::{self, JournalRow};
use sirtone_transfer::merge;
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
                context,
            } => Event {
                key: format!("{uid}:play:{id}"),
                track_key: key(track_id),
                ts,
                kind: EventKind::Play {
                    played_ms: played_ms.clamp(0, i64::from(u32::MAX)) as u32,
                    duration_ms: duration_ms.clamp(0, i64::from(u32::MAX)) as u32,
                    end_reason,
                    source,
                    context: Some(sirtone_transfer::PlayContext {
                        local_minute: context.local_minute,
                        weekday: context.weekday,
                        daypart: context.daypart,
                        season_w: context.season_w,
                        weather: context.weather,
                    }),
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

pub fn import(root: &str, pkg: &str) -> ExitCode {
    match run_import(Path::new(root), Path::new(pkg)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("transfer import: {e}");
            ExitCode::FAILURE
        }
    }
}

fn local_tracks(conn: &rusqlite::Connection) -> Result<Vec<merge::LocalTrack>, String> {
    Ok(transfer::tracks(conn)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| merge::LocalTrack {
            id: t.id,
            duration_ms: t.duration_ms,
            artist: t.artist,
            title: Some(t.title),
            fingerprint: t
                .fingerprint
                .as_deref()
                .and_then(sirtone_core::fingerprint::decompress),
        })
        .collect())
}

fn run_import(root: &Path, pkg: &Path) -> Result<(), String> {
    let manifest = Manifest::read(pkg).map_err(|e| e.to_string())?;
    let events = match std::fs::File::open(pkg.join(EVENTS_FILE)) {
        Ok(f) => sirtone_transfer::events::read_jsonl(std::io::BufReader::new(f))
            .map_err(|e| e.to_string())?,
        Err(_) => Vec::new(),
    };

    // 1. Which package tracks the library already has (by sound, else by tags).
    let conn = crate::library::open_existing(root)?;
    let matches = merge::match_tracks(&manifest.tracks, &local_tracks(&conn)?);
    drop(conn);
    let new_files: Vec<String> = manifest
        .tracks
        .iter()
        .zip(&matches)
        .filter(|(_, m)| **m == merge::TrackMatch::New)
        .map(|(t, _)| {
            t.file
                .split('/')
                .fold(pkg.to_path_buf(), |p, part| p.join(part))
        })
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    // 2. New music: the ordinary import (copy, recognition, layout); audio is never re-encoded.
    if !new_files.is_empty()
        && crate::library::import(&root.to_string_lossy(), &new_files) != ExitCode::SUCCESS
    {
        return Err("import of the new music failed".into());
    }

    // 3. Package keys → local tracks (imported ones are found again by tags; their
    //    fingerprints come later with the background analysis).
    let mut conn = crate::library::open_existing(root)?;
    let s = |e: sirtone_library::LibraryError| e.to_string();
    let local = local_tracks(&conn)?;
    let again = merge::match_tracks(&manifest.tracks, &local);
    let ids: HashMap<&str, i64> = manifest
        .tracks
        .iter()
        .zip(again)
        .filter_map(|(t, m)| match m {
            merge::TrackMatch::Same(id) => Some((t.key.as_str(), id)),
            merge::TrackMatch::New => None,
        })
        .collect();

    // 4. Plays and marks, once per key; for ♡ the package's latest mark of a track is applied
    //    when it changes the state here (D-140).
    let seen = transfer::seen_keys(&conn).map_err(s)?;
    // This library's own events that travelled there and back are already here.
    let own = format!("{}:", transfer::library_uid(&conn).map_err(s)?);
    let fresh: Vec<Event> = merge::new_events(&seen, &events)
        .into_iter()
        .filter(|e| !e.key.starts_with(&own))
        .cloned()
        .collect();
    // ♡ here: state and time of the latest local like/unlike of every track the package marks.
    let mut here: HashMap<String, (bool, i64)> = HashMap::new();
    for e in &fresh {
        if let (EventKind::Mark { .. }, Some(&track)) = (&e.kind, ids.get(e.track_key.as_str())) {
            let last: Option<(String, i64)> = conn
                .query_row(
                    "SELECT kind, ts FROM feedback WHERE track_id = ?1 AND kind IN ('like','unlike')
                      ORDER BY id DESC LIMIT 1",
                    [track],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
            if let Some((kind, ts)) = last {
                here.insert(e.track_key.clone(), (kind == "like", ts));
            }
        }
    }
    let latest = merge::likes_after_merge(&here, &fresh);
    let (mut plays, mut marks, mut skipped) = (0u32, 0u32, 0u32);
    for e in &fresh {
        let Some(&track) = ids.get(e.track_key.as_str()) else {
            skipped += 1;
            continue;
        };
        match &e.kind {
            EventKind::Play {
                played_ms,
                duration_ms,
                end_reason,
                source,
                context,
            } => {
                // A package without context (older format): an ordinary autumn-neutral day.
                let c = context.clone().unwrap_or(sirtone_transfer::PlayContext {
                    local_minute: 720,
                    weekday: 0,
                    daypart: 2,
                    season_w: [64, 64, 64, 63],
                    weather: None,
                });
                let ok = transfer::apply_play(
                    &conn,
                    &e.key,
                    track,
                    e.ts,
                    u64::from(*played_ms),
                    u64::from(*duration_ms),
                    end_reason,
                    source,
                    &transfer::PlayContext {
                        local_minute: c.local_minute,
                        weekday: c.weekday,
                        daypart: c.daypart,
                        season_w: c.season_w,
                        weather: c.weather,
                    },
                )
                .map_err(s)?;
                plays += u32::from(ok);
            }
            EventKind::Mark { kind } => {
                // `latest` holds only the marks that change the state here.
                let write_like = latest.get(&e.track_key).is_some_and(|(liked, ts)| {
                    *ts == e.ts
                        && matches!((kind.as_str(), liked), ("like", true) | ("unlike", false))
                });
                let ok = transfer::apply_mark(&conn, &e.key, track, e.ts, kind, write_like)
                    .map_err(s)?;
                marks += u32::from(ok);
            }
        }
    }

    // 5. Playlists: the same name gets the missing tracks; otherwise a new playlist.
    let existing = sirtone_library::playlists::list(&conn).map_err(s)?;
    let mut lists = 0u32;
    for p in &manifest.playlists {
        let tracks: Vec<i64> = p
            .tracks
            .iter()
            .filter_map(|k| ids.get(k.as_str()).copied())
            .collect();
        if tracks.is_empty() {
            continue;
        }
        let id = match existing.iter().find(|e| !e.smart && e.name == p.name) {
            Some(e) => e.id,
            None => sirtone_library::playlists::create(&conn, &p.name).map_err(s)?,
        };
        let have: HashSet<i64> = sirtone_library::playlists::tracks(&conn, id)
            .map_err(s)?
            .iter()
            .map(|t| t.track.id)
            .collect();
        let add: Vec<i64> = tracks.into_iter().filter(|t| !have.contains(t)).collect();
        if !add.is_empty() {
            sirtone_library::playlists::add(&mut conn, id, &add).map_err(s)?;
            lists += 1;
        }
    }
    println!(
        "package {}: {} tracks ({} new files imported), {plays} plays, {marks} marks, {lists} playlists updated, {skipped} events without a track",
        manifest.package_id,
        manifest.tracks.len(),
        new_files.len(),
    );
    Ok(())
}
