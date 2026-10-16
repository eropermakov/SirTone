//! Playback bridge (prototype 0.2): the audio engine behind typed IPC commands and events.
//! One entry point — [`open_paths`] — is fed by the native drop, the command line and a second
//! launch («Открыть с помощью»), so all three behave the same (S-03).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sirtone_audio::{EngineCmd, EngineEvent, EngineHandle, PlayState};
use sirtone_core::error::{AppError, ErrorCode};
use tauri::{AppHandle, State};
use tauri_specta::Event;

/// Extensions the prototype opens (docs/04 §2 minus WMA/APE/WV, which need T1.2).
const PLAYABLE: &[&str] = &[
    "mp3", "flac", "wav", "wave", "m4a", "mp4", "m4b", "aac", "ogg", "oga", "opus", "aif", "aiff",
    "caf",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackState {
    Empty,
    Playing,
    Paused,
    Ended,
}

/// Engine state for the player bar. Emitted on every change and every 250 ms while playing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSnapshot {
    pub state: PlaybackState,
    /// File name without extension until tags arrive (T1.15).
    pub title: Option<String>,
    pub position_ms: u32,
    pub duration_ms: Option<u32>,
    /// Slider value 0…1.
    pub volume: f32,
}

/// A track or device error; the UI shows `errors.<code>` in a toast (DESIGN.md §9).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackError {
    pub error: AppError,
    pub file: Option<String>,
}

pub struct Playback {
    engine: EngineHandle,
    last: Arc<Mutex<PlaybackSnapshot>>,
}

fn empty_snapshot() -> PlaybackSnapshot {
    PlaybackSnapshot {
        state: PlaybackState::Empty,
        title: None,
        position_ms: 0,
        duration_ms: None,
        volume: 1.0,
    }
}

fn file_name(p: &Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().into_owned())
}

impl Playback {
    pub fn start(app: AppHandle) -> Result<Self, AppError> {
        let last = Arc::new(Mutex::new(empty_snapshot()));
        let sink_last = last.clone();
        let engine = sirtone_audio::start(Box::new(move |ev| match ev {
            EngineEvent::Snapshot(s) => {
                let snap = PlaybackSnapshot {
                    state: match s.state {
                        PlayState::Empty => PlaybackState::Empty,
                        PlayState::Playing => PlaybackState::Playing,
                        PlayState::Paused => PlaybackState::Paused,
                        PlayState::Ended => PlaybackState::Ended,
                    },
                    title: s
                        .path
                        .as_deref()
                        .and_then(|p| p.file_stem())
                        .map(|n| n.to_string_lossy().into_owned()),
                    position_ms: u32::try_from(s.position_ms).unwrap_or(u32::MAX),
                    duration_ms: s.duration_ms.map(|d| u32::try_from(d).unwrap_or(u32::MAX)),
                    volume: s.volume,
                };
                if let Ok(mut l) = sink_last.lock() {
                    if *l == snap {
                        return;
                    }
                    *l = snap.clone();
                }
                let _ = snap.emit(&app);
            }
            EngineEvent::Error { error, path } => {
                tracing::warn!(target: "sirtone::playback", code = ?error.code, "playback error");
                let _ = PlaybackError {
                    error,
                    file: path.as_deref().and_then(file_name),
                }
                .emit(&app);
            }
        }))?;
        Ok(Self { engine, last })
    }

    /// Plays the first playable file among `paths` (folders and playlists come with import, T1.x).
    pub fn open_paths(&self, app: &AppHandle, paths: &[PathBuf]) {
        let first = paths.iter().find(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| PLAYABLE.contains(&e.to_ascii_lowercase().as_str()))
        });
        match first {
            Some(p) => self.engine.send(EngineCmd::Load {
                path: p.clone(),
                autoplay: true,
            }),
            None if !paths.is_empty() => {
                let _ = PlaybackError {
                    error: AppError::new(ErrorCode::UnsupportedFormat),
                    file: paths.first().and_then(|p| file_name(p)),
                }
                .emit(app);
            }
            None => {}
        }
    }

    pub fn shutdown(&self) {
        self.engine.send(EngineCmd::Shutdown);
    }
}

/// Paths from a command line: everything after the exe that is not a `--flag`.
pub fn paths_from_args(args: &[String]) -> Vec<PathBuf> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn playback_snapshot(pb: State<'_, Playback>) -> PlaybackSnapshot {
    pb.last
        .lock()
        .map(|s| s.clone())
        .unwrap_or_else(|_| empty_snapshot())
}

#[tauri::command]
#[specta::specta]
pub fn playback_open(app: AppHandle, pb: State<'_, Playback>, paths: Vec<String>) {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    pb.open_paths(&app, &paths);
}

#[tauri::command]
#[specta::specta]
pub fn playback_toggle(pb: State<'_, Playback>) {
    pb.engine.send(EngineCmd::Toggle);
}

#[tauri::command]
#[specta::specta]
pub fn playback_seek(pb: State<'_, Playback>, position_ms: u32) {
    pb.engine.send(EngineCmd::Seek(u64::from(position_ms)));
}

#[tauri::command]
#[specta::specta]
pub fn playback_set_volume(pb: State<'_, Playback>, volume: f32) {
    pb.engine.send(EngineCmd::SetVolume(volume));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_skip_exe_and_flags() {
        let args = ["sirtone.exe", "--debug", "D:\\Музыка\\трек 🎵.flac"].map(String::from);
        assert_eq!(
            paths_from_args(&args),
            [PathBuf::from("D:\\Музыка\\трек 🎵.flac")]
        );
    }
}
