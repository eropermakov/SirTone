//! Playback bridge (prototype 0.2): the audio engine behind typed IPC commands and events.
//! One entry point — [`open_paths`] — is fed by the native drop, the command line and a second
//! launch («Открыть с помощью»), so all three behave the same (S-03).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use sirtone_audio::{EngineCmd, EngineEvent, EngineHandle, PlayState};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::wave::{EndReason, PlaySource};
use sirtone_playback::{Queue, Step};
use tauri::{AppHandle, State};

use crate::journal::{Item as JournalItem, Journal};
use crate::settings_store::SettingsStore;
use tauri_specta::Event;

/// Extensions the player opens (docs/04 §2; APE/WV/TTA/MPC need the optional decoder pack).
const PLAYABLE: &[&str] = &[
    "mp3", "flac", "wav", "wave", "m4a", "mp4", "m4b", "aac", "ogg", "oga", "opus", "aif", "aiff",
    "wma", "caf",
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
    /// Track title from the library, or the file name for a file opened directly.
    pub title: Option<String>,
    pub artists: Option<String>,
    /// Library track id when playing from the collection.
    pub track_id: Option<u32>,
    pub has_next: bool,
    pub has_prev: bool,
    /// Album cover hash of the current track.
    pub cover: Option<String>,
    /// Output underruns since start (diagnostics, P-13).
    pub underruns: u32,
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

/// One queue entry: what the engine loads and what the player bar shows.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueItem {
    pub path: PathBuf,
    pub title: String,
    pub artists: Option<String>,
    pub track_id: Option<u32>,
    pub cover: Option<String>,
    /// Where playback was started from (journal, T1.31).
    pub source: PlaySource,
}

/// The screen a list was played from (`play_event.source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum PlayContext {
    Album,
    Artist,
    Collection,
}

impl From<PlayContext> for PlaySource {
    fn from(c: PlayContext) -> Self {
        match c {
            PlayContext::Album => PlaySource::Album,
            PlayContext::Artist => PlaySource::Artist,
            PlayContext::Collection => PlaySource::Collection,
        }
    }
}

pub struct Playback {
    engine: EngineHandle,
    settings: Arc<SettingsStore>,
    last: Arc<Mutex<PlaybackSnapshot>>,
    queue: Arc<Mutex<Queue<QueueItem>>>,
    journal: Arc<Journal>,
}

fn empty_snapshot() -> PlaybackSnapshot {
    PlaybackSnapshot {
        state: PlaybackState::Empty,
        title: None,
        artists: None,
        track_id: None,
        has_next: false,
        has_prev: false,
        cover: None,
        underruns: 0,
        position_ms: 0,
        duration_ms: None,
        volume: 1.0,
    }
}

fn file_name(p: &Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().into_owned())
}

impl Playback {
    pub fn start(app: AppHandle, settings: Arc<SettingsStore>) -> Result<Self, AppError> {
        let last = Arc::new(Mutex::new(empty_snapshot()));
        let sink_last = last.clone();
        let queue: Arc<Mutex<Queue<QueueItem>>> = Arc::new(Mutex::new(Queue::new()));
        let sink_queue = queue.clone();
        // The sink needs the handle to auto-advance; it exists only after `start` returns.
        let handle: Arc<OnceLock<EngineHandle>> = Arc::new(OnceLock::new());
        let sink_handle = handle.clone();
        let journal = Arc::new(Journal::start(settings.clone()));
        let sink_journal = journal.clone();
        let engine = sirtone_audio::start(Box::new(move |ev| match ev {
            EngineEvent::Snapshot(s) => {
                let mut q = sink_queue.lock().unwrap_or_else(|p| p.into_inner());
                // Gapless (T1.5): the engine moved on to the track we handed it in advance.
                if q.peek_next()
                    .is_some_and(|n| Some(n.path.as_path()) == s.path.as_deref())
                {
                    q.advance();
                    if let Some(h) = sink_handle.get() {
                        h.send(EngineCmd::SetNext(q.peek_next().map(|i| i.path.clone())));
                    }
                }
                sink_journal.observe(
                    &s,
                    q.current()
                        .filter(|c| Some(c.path.as_path()) == s.path.as_deref())
                        .map(|c| JournalItem {
                            path: &c.path,
                            track_id: c.track_id,
                            source: c.source,
                        }),
                );
                // End of a track with more in the context: the next one (FR-PLAY-005).
                if s.state == PlayState::Ended
                    && q.current()
                        .is_some_and(|c| Some(c.path.as_path()) == s.path.as_deref())
                    && let Step::Play(next) = q.advance()
                    && let Some(h) = sink_handle.get()
                {
                    h.send(EngineCmd::Load {
                        path: next.path,
                        autoplay: true,
                    });
                    h.send(EngineCmd::SetNext(q.peek_next().map(|i| i.path.clone())));
                    return;
                }
                let current = q
                    .current()
                    .filter(|c| Some(c.path.as_path()) == s.path.as_deref())
                    .cloned();
                let (has_next, has_prev) = (q.has_next(), q.has_prev());
                drop(q);
                let snap = PlaybackSnapshot {
                    state: match s.state {
                        PlayState::Empty => PlaybackState::Empty,
                        PlayState::Playing => PlaybackState::Playing,
                        PlayState::Paused => PlaybackState::Paused,
                        PlayState::Ended => PlaybackState::Ended,
                    },
                    title: current.as_ref().map(|c| c.title.clone()).or_else(|| {
                        s.path
                            .as_deref()
                            .and_then(|p| p.file_stem())
                            .map(|n| n.to_string_lossy().into_owned())
                    }),
                    artists: current.as_ref().and_then(|c| c.artists.clone()),
                    track_id: current.as_ref().and_then(|c| c.track_id),
                    has_next: current.is_some() && has_next,
                    has_prev: current.is_some() && has_prev,
                    cover: current.as_ref().and_then(|c| c.cover.clone()),
                    underruns: u32::try_from(s.underruns).unwrap_or(u32::MAX),
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
                sink_journal.on_error(path.as_deref());
                tracing::warn!(target: "sirtone::playback", code = ?error.code, "playback error");
                let _ = PlaybackError {
                    error,
                    file: path.as_deref().and_then(file_name),
                }
                .emit(&app);
            }
        }))?;
        let volume = settings.get()["playback"]["volume"].as_f64().unwrap_or(1.0) as f32;
        engine.send(EngineCmd::SetVolume(volume));
        // FR-PLAY-012: pause when the output device disappears (setting, default on).
        let pause_on_loss = settings.get()["playback"]["output"]["pause_on_device_loss"]
            .as_bool()
            .unwrap_or(true);
        engine.send(EngineCmd::SetPauseOnDeviceLoss(pause_on_loss));
        let _ = handle.set(engine.clone());
        Ok(Self {
            engine,
            settings,
            last,
            queue,
            journal,
        })
    }

    /// Plays the first playable file among `paths`; a folder plays its first playable file by
    /// name (import and queues come with T1.x).
    pub fn open_paths(&self, app: &AppHandle, paths: &[PathBuf]) {
        let first = paths.iter().find_map(|p| first_playable(p, 4));
        match first {
            Some(p) => {
                let title = p
                    .file_stem()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let item = QueueItem {
                    path: p,
                    title,
                    artists: None,
                    track_id: None,
                    cover: None,
                    source: PlaySource::External,
                };
                self.play_queue(vec![item], 0);
            }
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

    fn queue(&self) -> std::sync::MutexGuard<'_, Queue<QueueItem>> {
        self.queue.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Tells the engine what follows the current track, so it continues without a gap.
    fn hand_next(&self) {
        let next = self.queue().peek_next().map(|i| i.path.clone());
        self.engine.send(EngineCmd::SetNext(next));
    }

    /// Plays `items` as the new context from `start` (FR-PLAY-006: replaces the queue).
    pub fn play_queue(&self, items: Vec<QueueItem>, start: usize) {
        self.journal.close(EndReason::Skip);
        if let Step::Play(item) = self.queue().replace(items, start) {
            self.engine.send(EngineCmd::Load {
                path: item.path,
                autoplay: true,
            });
        }
        self.hand_next();
    }

    pub fn next(&self) {
        self.journal.close(EndReason::Skip);
        if let Step::Play(item) = self.queue().advance() {
            self.engine.send(EngineCmd::Load {
                path: item.path,
                autoplay: true,
            });
        }
        self.hand_next();
    }

    pub fn prev(&self) {
        self.journal.close(EndReason::Skip);
        match self.queue().back() {
            Step::Play(item) => self.engine.send(EngineCmd::Load {
                path: item.path,
                autoplay: true,
            }),
            Step::Restart => self.engine.send(EngineCmd::Seek(0)),
            Step::End => {}
        }
        self.hand_next();
    }

    pub fn snapshot(&self) -> PlaybackSnapshot {
        self.last
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| empty_snapshot())
    }

    pub fn toggle(&self) {
        self.engine.send(EngineCmd::Toggle);
    }

    pub fn shutdown(&self) {
        self.journal.shutdown();
        self.engine.send(EngineCmd::Shutdown);
    }
}

fn is_playable(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PLAYABLE.contains(&e.to_ascii_lowercase().as_str()))
}

/// `p` itself if playable, or the first playable file inside a folder (files before subfolders,
/// both by name, at most `depth` levels down).
fn first_playable(p: &Path, depth: u32) -> Option<PathBuf> {
    if p.is_file() {
        return is_playable(p).then(|| p.to_path_buf());
    }
    if depth == 0 || !p.is_dir() {
        return None;
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(p)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort_by_key(|e| (e.is_dir(), e.file_name().map(|n| n.to_ascii_lowercase())));
    entries.iter().find_map(|e| first_playable(e, depth - 1))
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
    let volume = if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        1.0
    };
    pb.engine.send(EngineCmd::SetVolume(volume));
    // Remembered between launches (D-64); the store debounces disk writes to 1/s.
    let _ = pb
        .settings
        .patch(&serde_json::json!({ "playback": { "volume": volume } }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_plays_its_first_playable_file() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/generated");
        if !dir.exists() {
            return;
        }
        let got = first_playable(&dir, 4).unwrap();
        assert!(is_playable(&got), "{got:?}");
        assert_eq!(first_playable(&dir.join("broken/playlist.m3u"), 4), None);
    }

    #[test]
    fn args_skip_exe_and_flags() {
        let args = ["sirtone.exe", "--debug", "D:\\Музыка\\трек 🎵.flac"].map(String::from);
        assert_eq!(
            paths_from_args(&args),
            [PathBuf::from("D:\\Музыка\\трек 🎵.flac")]
        );
    }
}

#[tauri::command]
#[specta::specta]
pub fn playback_next(pb: State<'_, Playback>) {
    pb.next();
}

#[tauri::command]
#[specta::specta]
pub fn playback_prev(pb: State<'_, Playback>) {
    pb.prev();
}

/// Plays library tracks `ids` as one context starting at `start` (double-click in a list).
#[tauri::command]
#[specta::specta]
pub fn play_tracks(
    pb: State<'_, Playback>,
    lib: State<'_, crate::library::Library>,
    ids: Vec<u32>,
    start: u32,
    context: PlayContext,
) -> Result<(), AppError> {
    let resolved = lib.resolve(&ids)?;
    let items = resolved
        .into_iter()
        .zip(ids.iter())
        .map(|((path, title, artists, cover), id)| QueueItem {
            path,
            title,
            artists: (!artists.is_empty()).then_some(artists),
            track_id: Some(*id),
            cover,
            source: context.into(),
        })
        .collect();
    pb.play_queue(items, start as usize);
    Ok(())
}
