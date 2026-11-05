//! `sirtone-cli transfer export <library_root> <out_dir> [--no-music]` — the transfer package of
//! a library (D-140, M6.2; check П7 uses it): `manifest.json`, `events.jsonl` and, unless
//! `--no-music`, the music under `music/`. Run again after a cut cable: only the missing or
//! short files are copied (resume by manifest). Audio is copied byte for byte.
//! `sirtone-cli transfer import <library_root> <package_dir>` — the other way (phone → PC): new
//! music goes through the ordinary import (recognition, layout, duplicates by content), then
//! plays, marks and playlists are applied once each (D-140). The work is `sirtone-sync`'s.
//! Run both on a COPY of a library (the database is opened writable: migrations go forward).

use std::path::Path;
use std::process::ExitCode;

use sirtone_transfer::{Parts, Platform};

pub fn export(root: &str, out: &str, music: bool) -> ExitCode {
    let root = Path::new(root);
    let run = || -> Result<sirtone_sync::ExportSummary, String> {
        let conn = crate::library::open_existing(root)?;
        let parts = Parts {
            music,
            playlists: true,
            taste: true,
            settings: false,
        };
        sirtone_sync::export(
            &conn,
            root,
            Path::new(out),
            Platform::Windows,
            env!("CARGO_PKG_VERSION"),
            parts,
        )
        .map_err(|e| e.to_string())
    };
    match run() {
        Ok(s) => {
            for f in &s.failed {
                eprintln!("{f}: not copied");
            }
            println!(
                "package {}: {} tracks ({} MB), {} playlists, {} events; copied now {} files ({} MB)",
                s.package_id,
                s.tracks,
                s.total_bytes / 1_000_000,
                s.playlists,
                s.events,
                s.copied,
                s.copied_bytes / 1_000_000,
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("transfer export: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn import(root: &str, pkg: &str) -> ExitCode {
    let root = Path::new(root);
    let run = || -> Result<sirtone_sync::ApplySummary, String> {
        let mut conn = crate::library::open_existing(root)?;
        sirtone_sync::apply(&mut conn, Path::new(pkg), &mut |db, files| {
            crate::library::import_into(db, root, files)
        })
        .map_err(|e| e.to_string())
    };
    match run() {
        Ok(s) => {
            println!(
                "package {}: {} tracks ({} new files imported), {} plays, {} marks, {} playlists updated, {} events without a track",
                s.package_id, s.tracks, s.new_files, s.plays, s.marks, s.playlists, s.skipped
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("transfer import: {e}");
            ExitCode::FAILURE
        }
    }
}
