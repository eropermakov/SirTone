//! Transfer package ⇄ library (D-134, D-140, `docs/mobile/README.md` §11): writes a library into
//! a package and applies a package to a library. A small composition crate over
//! `sirtone-library` (rows, journal, apply) and `sirtone-transfer` (format, merge rules) — used
//! by the PC hub, the phone core (`sirtone-mobile-ffi`) and `sirtone-cli`. Importing new music
//! is the caller's (its platform ports): it is handed in as a function.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use sirtone_library::LibraryError;
use sirtone_library::transfer::{self, JournalRow};
use sirtone_transfer::manifest::PlaylistEntry;
use sirtone_transfer::{
    EVENTS_FILE, Event, EventKind, MUSIC_DIR, Manifest, Parts, Platform, PlayContext, Source,
    TrackEntry, TransferError, merge,
};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("library: {0}")]
    Library(#[from] LibraryError),
    #[error("package: {0}")]
    Package(#[from] TransferError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("import of the new music failed: {0}")]
    Import(String),
}

impl SyncError {
    /// Stable code for the IPC/FFI boundary; the UI writes the words.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Library(_) => "transfer.library",
            Self::Package(e) => e.code(),
            Self::Io(_) => "transfer.io",
            Self::Import(_) => "transfer.import",
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportSummary {
    pub package_id: String,
    pub tracks: u32,
    pub total_bytes: u64,
    pub playlists: u32,
    pub events: u32,
    /// Files copied by this run (a resumed run copies only what was missing).
    pub copied: u32,
    pub copied_bytes: u64,
    /// Files that could not be read (moved away since the last scan); the rest went on.
    pub failed: Vec<String>,
}

/// Writes the package of the library at `root` (open as `conn`) into `out`. Run again after a
/// cut cable: the same package resumes and only missing or short files are copied.
pub fn export(
    conn: &Connection,
    root: &Path,
    out: &Path,
    platform: Platform,
    app_version: &str,
    parts: Parts,
) -> Result<ExportSummary, SyncError> {
    export_with(conn, root, out, platform, app_version, parts, parts.music)
}

/// As [`export`]; with `copy_music` false the manifest still lists the music (so the other side
/// imports it) but the files are not copied into `out` — the hub sends them to the phone
/// straight from the library ([`music_sources`]) instead of doubling them on the PC's disk.
pub fn export_with(
    conn: &Connection,
    root: &Path,
    out: &Path,
    platform: Platform,
    app_version: &str,
    parts: Parts,
    copy_music: bool,
) -> Result<ExportSummary, SyncError> {
    let uid = transfer::library_uid(conn)?;
    let key = |track: i64| format!("{uid}:{track}");

    let manifest = match Manifest::read(out) {
        Ok(m) if m.source.library_uuid == uid => m,
        _ => {
            let mut m = Manifest::new(
                format!("{uid}:{}", now_ms()),
                now_ms(),
                Source {
                    platform,
                    library_uuid: uid.clone(),
                    app_version: app_version.to_owned(),
                },
                parts,
            );
            for t in transfer::tracks(conn)? {
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
                    hash: Some(t.content_hash),
                    copy: t.copy,
                });
            }
            if parts.playlists {
                for p in sirtone_library::playlists::list(conn)? {
                    if p.smart {
                        continue; // rules, not lists: the other side rebuilds them itself
                    }
                    let tracks = sirtone_library::playlists::tracks(conn, p.id)?;
                    m.playlists.push(PlaylistEntry {
                        name: p.name,
                        tracks: tracks.iter().map(|t| key(t.track.id)).collect(),
                    });
                }
            }
            m.validate()?;
            m.write(out)?;
            m
        }
    };

    let events: Vec<Event> = if parts.taste {
        transfer::journal(conn)?
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
                        context: Some(PlayContext {
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
            .collect()
    } else {
        Vec::new()
    };
    let mut buf = Vec::new();
    sirtone_transfer::events::write_jsonl(&mut buf, &events)?;
    std::fs::write(out.join(EVENTS_FILE), buf)?;

    let mut sum = ExportSummary {
        package_id: manifest.package_id.clone(),
        tracks: manifest.tracks.len() as u32,
        total_bytes: manifest.total_bytes(),
        playlists: manifest.playlists.len() as u32,
        events: events.len() as u32,
        ..ExportSummary::default()
    };
    if parts.music && copy_music {
        let at = |p: &str| p.split('/').fold(out.to_path_buf(), |d, part| d.join(part));
        let size_at = |p: &str| std::fs::metadata(at(p)).ok().map(|m| m.len());
        let pending: Vec<TrackEntry> = manifest.pending(size_at).into_iter().cloned().collect();
        for t in &pending {
            let rel = t.file.strip_prefix("music/").unwrap_or(&t.file);
            let src = rel
                .split('/')
                .fold(root.to_path_buf(), |p, part| p.join(part));
            let dst = at(&t.file);
            if let Some(dir) = dst.parent() {
                std::fs::create_dir_all(dir)?;
            }
            // `.part` then rename: a cut cable never leaves a short file under the real name.
            let part = dst.with_extension("sirtone-part");
            match std::fs::copy(&src, &part).and_then(|_| std::fs::rename(&part, &dst)) {
                Ok(()) => {
                    sum.copied += 1;
                    sum.copied_bytes += t.size;
                }
                Err(_) => sum.failed.push(t.file.clone()),
            }
        }
    }
    Ok(sum)
}

/// The music of a package written by [`export_with`] without copying: (file in the library,
/// its path inside the package) for every listed track that is not a copy made into the
/// package (those files are in `out` already).
pub fn music_sources(root: &Path, out: &Path) -> Result<Vec<(PathBuf, String)>, SyncError> {
    let m = Manifest::read(out)?;
    if !m.parts.music {
        return Ok(Vec::new());
    }
    Ok(m.tracks
        .iter()
        // A copy made into the package is already there (D-146).
        .filter(|t| t.copy.is_none())
        .map(|t| {
            let rel = t.file.strip_prefix("music/").unwrap_or(&t.file);
            let src = rel
                .split('/')
                .fold(root.to_path_buf(), |p, part| p.join(part));
            (src, t.file.clone())
        })
        .collect())
}

/// The caller's import of new music files into the open library (its platform ports).
pub type ImportNew<'a> = dyn FnMut(&mut Connection, &[PathBuf]) -> Result<(), String> + 'a;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplySummary {
    pub package_id: String,
    pub tracks: u32,
    /// Package files handed to the import (music this library did not have).
    pub new_files: u32,
    pub plays: u32,
    pub marks: u32,
    pub playlists: u32,
    /// Events whose track is neither here nor imported (e.g. a file left out of the package).
    pub skipped: u32,
}

fn local_tracks(conn: &Connection) -> Result<Vec<merge::LocalTrack>, LibraryError> {
    Ok(transfer::tracks(conn)?
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
            hash: Some(t.content_hash),
        })
        .collect())
}

/// Applies the package in `pkg` to the library open as `conn`: new music through
/// `import_new(conn, files)` (the caller's ordinary import: copy, recognition, layout,
/// duplicates by content), then plays, marks and playlists — each key once (D-140).
pub fn apply(
    conn: &mut Connection,
    pkg: &Path,
    import_new: &mut ImportNew<'_>,
) -> Result<ApplySummary, SyncError> {
    let manifest = Manifest::read(pkg)?;
    let events = match std::fs::File::open(pkg.join(EVENTS_FILE)) {
        Ok(f) => sirtone_transfer::events::read_jsonl(std::io::BufReader::new(f))?,
        Err(_) => Vec::new(),
    };

    // 1. Which package tracks the library already has (by sound, else by tags).
    let matches = merge::match_tracks(&manifest.tracks, &local_tracks(conn)?);
    let new_files: Vec<PathBuf> = manifest
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
        .collect();

    // 2. New music: the ordinary import; audio is never re-encoded.
    if !new_files.is_empty() {
        import_new(conn, &new_files).map_err(SyncError::Import)?;
    }

    // 3. Package keys → local tracks (imported ones are found again by tags; their
    //    fingerprints come later with the background analysis).
    let again = merge::match_tracks(&manifest.tracks, &local_tracks(conn)?);
    let ids: HashMap<&str, i64> = manifest
        .tracks
        .iter()
        .zip(again)
        .filter_map(|(t, m)| match m {
            merge::TrackMatch::Same(id) => Some((t.key.as_str(), id)),
            merge::TrackMatch::New => None,
        })
        .collect();

    // 3b. Music that came as an economy copy stays marked so (Q-34): only tracks this run
    //     imported — a track the library already had keeps its own file.
    for ((t, first), m) in manifest
        .tracks
        .iter()
        .zip(&matches)
        .zip(merge::match_tracks(&manifest.tracks, &local_tracks(conn)?))
    {
        if let (Some(level), merge::TrackMatch::New, merge::TrackMatch::Same(id)) =
            (t.copy.as_deref(), first, m)
        {
            sirtone_library::copies::mark_copy_only(conn, id, level)?;
        }
    }

    // 4. Plays and marks, once per key; this library's own events that travelled there and back
    //    are already here; for ♡ the latest mark wins (D-140).
    let seen = transfer::seen_keys(conn)?;
    let own = format!("{}:", transfer::library_uid(conn)?);
    let fresh: Vec<Event> = merge::new_events(&seen, &events)
        .into_iter()
        .filter(|e| !e.key.starts_with(&own))
        .cloned()
        .collect();
    let mut here: HashMap<String, (bool, i64)> = HashMap::new();
    for e in &fresh {
        if let (EventKind::Mark { .. }, Some(&track)) = (&e.kind, ids.get(e.track_key.as_str()))
            && let Ok((kind, ts)) = conn.query_row(
                "SELECT kind, ts FROM feedback WHERE track_id = ?1 AND kind IN ('like','unlike')
                  ORDER BY id DESC LIMIT 1",
                [track],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
        {
            here.insert(e.track_key.clone(), (kind == "like", ts));
        }
    }
    let latest = merge::likes_after_merge(&here, &fresh);
    let mut sum = ApplySummary {
        package_id: manifest.package_id.clone(),
        tracks: manifest.tracks.len() as u32,
        new_files: new_files.len() as u32,
        ..ApplySummary::default()
    };
    for e in &fresh {
        let Some(&track) = ids.get(e.track_key.as_str()) else {
            sum.skipped += 1;
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
                // A package without context (older format): an ordinary day, no season.
                let c = context.clone().unwrap_or(PlayContext {
                    local_minute: 720,
                    weekday: 0,
                    daypart: 2,
                    season_w: [64, 64, 64, 63],
                    weather: None,
                });
                let ok = transfer::apply_play(
                    conn,
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
                )?;
                sum.plays += u32::from(ok);
            }
            EventKind::Mark { kind } => {
                // `latest` holds only the ♡ marks that change the state here.
                let write_like = latest.get(&e.track_key).is_some_and(|(liked, ts)| {
                    *ts == e.ts
                        && matches!((kind.as_str(), liked), ("like", true) | ("unlike", false))
                });
                let ok = transfer::apply_mark(conn, &e.key, track, e.ts, kind, write_like)?;
                sum.marks += u32::from(ok);
            }
        }
    }

    // 5. Playlists: the same name gets the missing tracks; otherwise a new playlist.
    let existing = sirtone_library::playlists::list(conn)?;
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
            None => sirtone_library::playlists::create(conn, &p.name)?,
        };
        let have: HashSet<i64> = sirtone_library::playlists::tracks(conn, id)?
            .iter()
            .map(|t| t.track.id)
            .collect();
        let add: Vec<i64> = tracks.into_iter().filter(|t| !have.contains(t)).collect();
        if !add.is_empty() {
            sirtone_library::playlists::add(conn, id, &add)?;
            sum.playlists += 1;
        }
    }
    Ok(sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sirtone_library::repo::{
        Identity, NewTrack, album_get_or_create, artist_get_or_create, track_insert,
    };

    /// The hub's way to the phone: the manifest lists the music, `out` holds no music, and the
    /// sources point into the library.
    #[test]
    fn export_without_copy_lists_music_in_place() {
        let base = std::env::temp_dir().join(format!("sirtone-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (root, out) = (base.join("lib"), base.join("out"));
        std::fs::create_dir_all(root.join("Кино")).unwrap();
        std::fs::write(root.join("Кино/01 Ёлка.flac"), b"flac").unwrap();
        let mut conn = sirtone_library::open(
            &root.join(".sirtone").join("library.db"),
            sirtone_library::Medium::Fixed,
        )
        .unwrap();
        let kino = artist_get_or_create(&conn, "Кино").unwrap();
        let album = album_get_or_create(&conn, "Ёлка", kino, None).unwrap();
        track_insert(
            &mut conn,
            &NewTrack {
                rel_path: "Кино/01 Ёлка.flac".into(),
                size_bytes: 4,
                mtime_ms: 0,
                content_hash: "h1".into(),
                container: "flac".into(),
                codec: "flac".into(),
                lossless: true,
                sample_rate: Some(44100),
                channels: Some(2),
                duration_ms: 1_000,
                title: "Ёлка".into(),
                album_id: Some(album),
                disc_no: None,
                track_no: Some(1),
                year: None,
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![kino],
            },
        )
        .unwrap();
        let parts = Parts {
            music: true,
            playlists: true,
            taste: true,
            settings: false,
        };
        let s = export_with(&conn, &root, &out, Platform::Windows, "t", parts, false).unwrap();
        assert_eq!((s.tracks, s.copied), (1, 0));
        assert!(
            !out.join(MUSIC_DIR).exists(),
            "no music copied into the package"
        );
        let src = music_sources(&root, &out).unwrap();
        assert_eq!(src.len(), 1);
        assert_eq!(src[0].0, root.join("Кино").join("01 Ёлка.flac"));
        assert_eq!(src[0].1, format!("{MUSIC_DIR}/Кино/01 Ёлка.flac"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
