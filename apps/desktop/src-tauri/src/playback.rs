//! Playback bridge (prototype 0.2): the audio engine behind typed IPC commands and events.
//! One entry point — [`open_paths`] — is fed by the native drop, the command line and a second
//! launch («Открыть с помощью»), so all three behave the same (S-03).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use sirtone_audio::{EngineCmd, EngineEvent, EngineHandle, PlayState};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::wave::{EndReason, PlaySource};
use sirtone_platform::smtc::{Smtc, SmtcButton, SmtcState, SmtcTrack};
use sirtone_playback::{Queue, Repeat, ShuffleKey, Step};
use tauri::{AppHandle, Manager, State};

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
    /// The current track comes from «Моя волна» (the screen offers to start it otherwise).
    pub in_wave: bool,
    /// The running wave's profile (`my_wave`, `seasonal`) while a wave track plays.
    pub wave_profile: Option<String>,
    pub has_next: bool,
    pub has_prev: bool,
    /// Album cover hash of the current track.
    pub cover: Option<String>,
    /// Placeholder hint when there is no cover (T3.20).
    pub art: u8,
    pub repeat: RepeatMode,
    pub shuffle: bool,
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
    /// Placeholder hint (T3.20).
    pub art: u8,
    /// Where playback was started from (journal, T1.31).
    pub source: PlaySource,
    /// Album and primary artist (shuffle spreading); 0 = unknown.
    pub album_key: u64,
    pub artist_key: u64,
}

/// FR-PLAY-007 repeat mode over IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    Off,
    One,
    All,
}

impl From<Repeat> for RepeatMode {
    fn from(r: Repeat) -> Self {
        match r {
            Repeat::Off => Self::Off,
            Repeat::One => Self::One,
            Repeat::All => Self::All,
        }
    }
}

impl From<RepeatMode> for Repeat {
    fn from(r: RepeatMode) -> Self {
        match r {
            RepeatMode::Off => Self::Off,
            RepeatMode::One => Self::One,
            RepeatMode::All => Self::All,
        }
    }
}

/// One row of the queue panel.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    pub title: String,
    pub artists: Option<String>,
    pub track_id: Option<u32>,
    pub cover: Option<String>,
    /// Placeholder hint (T3.20).
    pub art: u8,
    /// Added by ∞ (shown under «Рекомендовано из вашей медиатеки» with «+»/«−»).
    pub recommended: bool,
}

/// The queue panel (FR-PLAY-006): everything, the playing position and the modes.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct QueueView {
    pub items: Vec<QueueEntry>,
    pub position: u32,
    pub repeat: RepeatMode,
    pub shuffle: bool,
    /// ∞ of the playing context (FR-PLS-005); `None` — not applicable (the wave, files).
    pub autoplay: Option<bool>,
    /// The playlist that is playing («+» adds there, ∞ is its own).
    pub playlist: Option<u32>,
}

/// The queue changed (order, contents, modes): the panel refetches.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct QueueChanged {
    pub len: u32,
}

/// What goes to the engine in advance for a gapless joint (repeat-one replays via Load instead).
fn handover(q: &Queue<QueueItem>) -> Option<&QueueItem> {
    if q.repeat() == Repeat::One {
        None
    } else {
        q.upcoming()
    }
}

/// The screen a list was played from (`play_event.source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum PlayContext {
    Album,
    Artist,
    Collection,
    Playlist,
    Search,
    Wave,
}

impl From<PlayContext> for PlaySource {
    fn from(c: PlayContext) -> Self {
        match c {
            PlayContext::Album => PlaySource::Album,
            PlayContext::Artist => PlaySource::Artist,
            PlayContext::Collection => PlaySource::Collection,
            PlayContext::Playlist => PlaySource::Playlist,
            PlayContext::Search => PlaySource::Search,
            PlayContext::Wave => PlaySource::Wave,
        }
    }
}

pub struct Playback {
    /// For events from user commands (queue edits); set once the app runs.
    app: OnceLock<AppHandle>,
    engine: EngineHandle,
    settings: Arc<SettingsStore>,
    last: Arc<Mutex<PlaybackSnapshot>>,
    queue: Arc<Mutex<Queue<QueueItem>>>,
    journal: Arc<Journal>,
    /// Volume before «без звука» (mute hotkey), restored by the next press.
    muted_from: Mutex<Option<f32>>,
    /// Keeps the media-controls session alive for the app's lifetime.
    _smtc: Option<Arc<Smtc>>,
}

/// Mirrors a snapshot into the Windows media controls: the track (with a cover built off the
/// engine thread) when it changes, the state and position always (throttled inside).
fn update_smtc(
    app: &AppHandle,
    smtc: &Arc<Smtc>,
    last: &Mutex<Option<(Option<String>, Option<String>)>>,
    generation: &Arc<std::sync::atomic::AtomicU64>,
    snap: &PlaybackSnapshot,
) {
    use std::sync::atomic::Ordering;
    let key = (snap.title.clone(), snap.cover.clone());
    let changed = last
        .lock()
        .map(|mut l| {
            let changed = l.as_ref() != Some(&key);
            *l = Some(key);
            changed
        })
        .unwrap_or(false);
    if changed {
        let track = snap.title.clone().map(|title| SmtcTrack {
            title,
            artist: snap.artists.clone().unwrap_or_default(),
            album: String::new(),
            cover: None,
            duration_ms: u64::from(snap.duration_ms.unwrap_or(0)),
        });
        // Fast skipping: a slow cover of an earlier track must not overwrite a later one.
        let mine = generation.fetch_add(1, Ordering::SeqCst) + 1;
        let generation = generation.clone();
        let (app, smtc, hash) = (app.clone(), smtc.clone(), snap.cover.clone());
        std::thread::spawn(move || {
            let track = track.map(|mut t| {
                t.cover = hash.and_then(|h| {
                    app.try_state::<crate::library::Library>()
                        .and_then(|l| l.smtc_cover(&h))
                });
                t
            });
            if generation.load(Ordering::SeqCst) == mine {
                smtc.set_track(track);
            }
        });
    }
    let state = match snap.state {
        PlaybackState::Playing => SmtcState::Playing,
        PlaybackState::Paused => SmtcState::Paused,
        PlaybackState::Empty | PlaybackState::Ended => SmtcState::Stopped,
    };
    smtc.set_state(
        state,
        u64::from(snap.position_ms),
        snap.has_next,
        snap.has_prev,
    );
}

fn empty_snapshot() -> PlaybackSnapshot {
    PlaybackSnapshot {
        state: PlaybackState::Empty,
        title: None,
        artists: None,
        track_id: None,
        in_wave: false,
        wave_profile: None,
        has_next: false,
        has_prev: false,
        cover: None,
        art: 0x70,
        repeat: RepeatMode::Off,
        shuffle: false,
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
        let app_for_events = app.clone();
        let journal = Arc::new(Journal::start(settings.clone()));
        let sink_journal = journal.clone();
        let wave_app = app.clone();
        // Media keys, the Windows 11 flyout and widgets (T1.23): buttons go straight to this
        // service, so they work with the window hidden.
        let smtc_app = app.clone();
        let smtc = Smtc::start(move |b| {
            let Some(pb) = smtc_app.try_state::<Playback>() else {
                return;
            };
            match b {
                // Explicit Play/Pause (headsets, lock screen, the play key after Windows maps it):
                // a stale status must never turn «Pause» into «play».
                SmtcButton::Play => pb.play(),
                SmtcButton::Pause => pb.pause(),
                SmtcButton::Toggle => pb.toggle(),
                SmtcButton::Next => pb.next(),
                SmtcButton::Previous => pb.prev(),
                SmtcButton::Stop => pb.pause(),
                SmtcButton::Seek(ms) => pb.engine.send(EngineCmd::Seek(ms)),
            }
        })
        .map(Arc::new);
        let sink_smtc = smtc.clone();
        let smtc_last: Mutex<Option<(Option<String>, Option<String>)>> = Mutex::new(None);
        let smtc_gen = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let engine = sirtone_audio::start(Box::new(move |ev| match ev {
            EngineEvent::Snapshot(s) => {
                let mut q = sink_queue.lock().unwrap_or_else(|p| p.into_inner());
                // Gapless (T1.5): the engine moved on to the track we handed it in advance.
                if q.current()
                    .is_none_or(|c| Some(c.path.as_path()) != s.path.as_deref())
                    && handover(&q).is_some_and(|n| Some(n.path.as_path()) == s.path.as_deref())
                {
                    q.advance();
                    if let Some(h) = sink_handle.get() {
                        h.send(EngineCmd::SetNext(handover(&q).map(|i| i.path.clone())));
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
                // «Моя волна» is endless: with fewer than 2 tracks ahead the engine adds more.
                if q.current().is_some_and(|c| c.source == PlaySource::Wave)
                    && q.len().saturating_sub(q.position() + 1) < 2
                    && let Some(w) = wave_app.try_state::<crate::wave::Wave>()
                {
                    w.request_more();
                }
                // ∞ (T3.14): a playlist or list near its end goes on with similar tracks.
                if q.current().is_some_and(|c| c.source != PlaySource::Wave)
                    && q.repeat() == Repeat::Off
                    && q.len().saturating_sub(q.position() + 1) < 2
                    && let Some(c) = wave_app.try_state::<crate::continuation::Continuation>()
                    && c.active()
                {
                    c.request_more(&wave_app);
                }
                // End of a track with more in the context: the next one (FR-PLAY-005).
                if s.state == PlayState::Ended
                    && q.current()
                        .is_some_and(|c| Some(c.path.as_path()) == s.path.as_deref())
                    && let Step::Play(next) = q.on_track_end()
                    && let Some(h) = sink_handle.get()
                {
                    h.send(EngineCmd::Load {
                        path: next.path,
                        autoplay: true,
                    });
                    h.send(EngineCmd::SetNext(handover(&q).map(|i| i.path.clone())));
                    return;
                }
                let current = q
                    .current()
                    .filter(|c| Some(c.path.as_path()) == s.path.as_deref())
                    .cloned();
                let (has_next, has_prev) = (q.has_next(), q.has_prev());
                let (repeat, shuffle) = (q.repeat().into(), q.shuffled());
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
                    in_wave: current
                        .as_ref()
                        .is_some_and(|c| c.source == PlaySource::Wave),
                    wave_profile: current
                        .as_ref()
                        .is_some_and(|c| c.source == PlaySource::Wave)
                        .then(|| sink_journal.wave_profile())
                        .flatten(),
                    has_next: current.is_some() && has_next,
                    has_prev: current.is_some() && has_prev,
                    cover: current.as_ref().and_then(|c| c.cover.clone()),
                    art: current.as_ref().map_or(0x70, |c| c.art),
                    repeat,
                    shuffle,
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
                if let Some(smtc) = sink_smtc.as_ref() {
                    update_smtc(&app, smtc, &smtc_last, &smtc_gen, &snap);
                }
                crate::tray::update(
                    &app,
                    &crate::tray::TrayState {
                        title: snap.title.clone(),
                        artists: snap.artists.clone(),
                        playing: snap.state == PlaybackState::Playing,
                        has_prev: snap.has_prev,
                        has_next: snap.has_next,
                    },
                );
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
            muted_from: Mutex::new(None),
            _smtc: smtc,
            app: {
                let cell = OnceLock::new();
                let _ = cell.set(app_for_events);
                cell
            },
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
                    art: 0x70,
                    source: PlaySource::External,
                    album_key: 0,
                    artist_key: 0,
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
        let next = handover(&self.queue()).map(|i| i.path.clone());
        self.engine.send(EngineCmd::SetNext(next));
    }

    /// After a queue edit: the engine gets the new «next», the player bar its flags, the panel
    /// a refetch event.
    fn queue_edited(&self) {
        self.hand_next();
        let (has_next, has_prev, repeat, shuffle, len) = {
            let q = self.queue();
            (
                q.has_next(),
                q.has_prev(),
                q.repeat().into(),
                q.shuffled(),
                q.len(),
            )
        };
        let snap = self.last.lock().ok().map(|mut l| {
            if l.title.is_some() {
                l.has_next = has_next;
                l.has_prev = has_prev;
            }
            l.repeat = repeat;
            l.shuffle = shuffle;
            l.clone()
        });
        if let Some(app) = self.app.get() {
            if let Some(s) = snap {
                let _ = s.emit(app);
            }
            let _ = QueueChanged { len: len as u32 }.emit(app);
        }
    }

    pub fn play_next(&self, items: Vec<QueueItem>) {
        let step = self.queue().play_next(items);
        self.start_if_new(step);
        self.queue_edited();
    }

    pub fn append(&self, items: Vec<QueueItem>) {
        let step = self.queue().append(items);
        self.start_if_new(step);
        self.queue_edited();
    }

    /// An empty queue starts playing what was added.
    fn start_if_new(&self, step: Step<QueueItem>) {
        if let Step::Play(item) = step {
            self.journal.close(EndReason::Skip);
            self.engine.send(EngineCmd::Load {
                path: item.path,
                autoplay: true,
            });
        }
    }

    /// The wave started with this profile (stamped on its plays in the journal).
    pub fn set_wave_profile(&self, id: &str) {
        self.journal.set_wave_profile(id);
    }

    /// Drops every not-yet-played wave item after the current one (the slider moved).
    pub fn drop_upcoming_wave(&self) -> usize {
        self.drop_upcoming(|i| i.source == PlaySource::Wave)
    }

    /// Drops the not-yet-played items after the current one that match.
    pub fn drop_upcoming(&self, pred: impl Fn(&QueueItem) -> bool) -> usize {
        let mut q = self.queue();
        let pos = q.position();
        let mut n = 0;
        for at in (pos + 1..q.len()).rev() {
            if q.items().get(at).is_some_and(&pred) && q.remove(at) {
                n += 1;
            }
        }
        drop(q);
        if n > 0 {
            self.queue_edited();
        }
        n
    }

    /// The library track that is playing (the online lookup does not move it under the engine).
    pub fn current_track_id(&self) -> Option<u32> {
        self.queue().current().and_then(|i| i.track_id)
    }

    /// Library files moved (assign, «навести порядок», artist merge, the online lookup): queued
    /// tracks get their new path and tags. The playing item keeps its path — the engine matches
    /// its events by it.
    pub fn refresh_from(&self, lib: &crate::library::Library) {
        let ids: Vec<u32> = {
            let q = self.queue();
            let pos = q.position();
            let mut ids: Vec<u32> = q
                .items()
                .iter()
                .enumerate()
                .filter(|(n, _)| *n != pos)
                .filter_map(|(_, i)| i.track_id)
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        if ids.is_empty() {
            return;
        }
        let Ok(fresh) = lib.resolve(&ids) else {
            return;
        };
        if apply_fresh(&mut self.queue(), &fresh) > 0 {
            self.queue_edited();
        }
    }

    /// Recommendations up to and including the current item (the continuation goes on from
    /// them).
    pub fn played_hybrid(&self) -> Vec<i64> {
        let q = self.queue();
        q.items()
            .iter()
            .take(q.position() + 1)
            .filter(|i| i.source == PlaySource::Hybrid)
            .filter_map(|i| i.track_id.map(i64::from))
            .collect()
    }

    /// «+»: queued recommendations of these tracks now belong to the playlist.
    pub fn promote(&self, ids: &[u32]) {
        let mut q = self.queue();
        let n = q.update(|i| {
            if i.source == PlaySource::Hybrid && i.track_id.is_some_and(|t| ids.contains(&t)) {
                i.source = PlaySource::Playlist;
                true
            } else {
                false
            }
        });
        drop(q);
        if n > 0 {
            self.queue_edited();
        }
    }

    /// The queue panel refetches (∞ switched).
    pub fn changed(&self) {
        self.queue_edited();
    }

    pub fn queue_remove(&self, at: usize) {
        if self.queue().remove(at) {
            self.queue_edited();
        }
    }

    /// Removed tracks (FR-DATA-001/002) leave the queue; the playing one plays on.
    pub fn forget_tracks(&self, ids: &[u32]) {
        let mut q = self.queue();
        let gone: Vec<usize> = q
            .items()
            .iter()
            .enumerate()
            .filter(|(_, i)| i.track_id.is_some_and(|t| ids.contains(&t)))
            .map(|(n, _)| n)
            .collect();
        let mut changed = false;
        for at in gone.into_iter().rev() {
            changed |= q.remove(at);
        }
        drop(q);
        if changed {
            self.queue_edited();
        }
    }

    pub fn queue_move(&self, from: usize, to: usize) {
        if self.queue().move_item(from, to) {
            self.queue_edited();
        }
    }

    pub fn queue_jump(&self, at: usize) {
        let step = self.queue().jump(at);
        self.start_if_new(step);
        self.queue_edited();
    }

    pub fn set_repeat(&self, r: Repeat) {
        self.queue().set_repeat(r);
        self.queue_edited();
    }

    /// Shuffle on/off (FR-PLAY-007): «Fewer Repeats» over the upcoming tracks; tracks heard in
    /// the last day go later.
    pub fn set_shuffle(&self, on: bool, recent: &std::collections::HashSet<i64>) {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        self.queue().set_shuffle(on, seed, |i| ShuffleKey {
            artist: i.artist_key,
            album: i.album_key,
            recent: i.track_id.is_some_and(|t| recent.contains(&i64::from(t))),
        });
        self.queue_edited();
    }

    pub fn queue_view(&self) -> QueueView {
        let q = self.queue();
        QueueView {
            items: q
                .items()
                .iter()
                .map(|i| QueueEntry {
                    title: i.title.clone(),
                    artists: i.artists.clone(),
                    track_id: i.track_id,
                    cover: i.cover.clone(),
                    art: i.art,
                    recommended: i.source == PlaySource::Hybrid,
                })
                .collect(),
            position: q.position() as u32,
            repeat: q.repeat().into(),
            shuffle: q.shuffled(),
            autoplay: None,
            playlist: None,
        }
    }

    /// Plays `items` as the new context from `start` (FR-PLAY-006: replaces the queue). The
    /// continuation of the previous context ends; `play_tracks` starts a new one.
    pub fn play_queue(&self, items: Vec<QueueItem>, start: usize) {
        if let Some(c) = self
            .app
            .get()
            .and_then(|a| a.try_state::<crate::continuation::Continuation>())
        {
            c.reset();
        }
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
        // A skipped wave track turns the wave away from its vibe at once (D-92, AT-21).
        let skipped = self
            .queue()
            .current()
            .filter(|c| c.source == PlaySource::Wave)
            .and_then(|c| c.track_id);
        if let (Some(t), Some(app)) = (skipped, self.app.get())
            && let Some(w) = app.try_state::<crate::wave::Wave>()
        {
            w.skipped(t);
        }
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

    /// A visualiser frame source on this engine (T4.1).
    pub fn viz_source(&self) -> sirtone_audio::viz::VizSource {
        self.engine.viz()
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

    pub fn pause(&self) {
        self.engine.send(EngineCmd::Pause);
    }

    pub fn play(&self) {
        self.engine.send(EngineCmd::Play);
    }

    /// Current player volume (0..1), as the engine last reported it.
    pub fn volume(&self) -> f32 {
        self.last.lock().map(|s| s.volume).unwrap_or(1.0)
    }

    /// Sets the player volume (only Сиртон's, never the system's) and remembers it (D-64).
    pub fn set_volume(&self, volume: f32) {
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if let Ok(mut m) = self.muted_from.lock() {
            *m = None;
        }
        self.engine.send(EngineCmd::SetVolume(volume));
        // Remembered between launches (D-64); the store debounces disk writes to 1/s.
        let _ = self
            .settings
            .patch(&serde_json::json!({ "playback": { "volume": volume } }));
    }

    /// «Без звука» and back to the previous volume.
    pub fn toggle_mute(&self) {
        let restore = self.muted_from.lock().ok().and_then(|mut m| m.take());
        match restore {
            Some(v) => self.set_volume(v),
            None => {
                let current = self.volume();
                self.engine.send(EngineCmd::SetVolume(0.0));
                if let Ok(mut m) = self.muted_from.lock() {
                    *m = Some(current);
                }
            }
        }
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

#[tauri::command(async)]
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
    pb.set_volume(volume);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib_item(id: u32, path: &str) -> QueueItem {
        QueueItem {
            path: PathBuf::from(path),
            title: format!("t{id}"),
            artists: None,
            track_id: Some(id),
            cover: None,
            art: 0x70,
            source: PlaySource::Collection,
            album_key: 0,
            artist_key: 0,
        }
    }

    #[test]
    fn moved_files_reach_the_queue_but_not_the_playing_item() {
        let mut q = Queue::new();
        q.replace(
            vec![
                lib_item(1, "D:\\m\\Неизвестные\\a.mp3"),
                lib_item(2, "D:\\m\\Неизвестные\\b.mp3"),
            ],
            0,
        );
        let fresh = |id: u32, path: &str| crate::library::Resolved {
            id,
            path: PathBuf::from(path),
            title: format!("T{id}"),
            artists: "Исполнитель".into(),
            cover: None,
            album_id: Some(7),
            artist_id: Some(3),
            art: 0x70,
        };
        let moved = [
            fresh(1, "D:\\m\\Исполнитель\\A.mp3"),
            fresh(2, "D:\\m\\Исполнитель\\B.mp3"),
        ];
        assert_eq!(apply_fresh(&mut q, &moved), 1);
        assert_eq!(
            q.current().map(|i| i.path.clone()),
            Some(PathBuf::from("D:\\m\\Неизвестные\\a.mp3")),
            "the engine matches the playing item by its path"
        );
        let next = handover(&q).cloned().unwrap();
        assert_eq!(next.path, PathBuf::from("D:\\m\\Исполнитель\\B.mp3"));
        assert_eq!(
            (next.title.as_str(), next.artists.as_deref(), next.album_key),
            ("T2", Some("Исполнитель"), 7)
        );
        assert_eq!(next.source, PlaySource::Collection);
        assert_eq!(
            apply_fresh(&mut q, &moved),
            0,
            "nothing new the second time"
        );
    }

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

/// Puts what the library now says into every queued item but the current one; returns how many
/// changed.
fn apply_fresh(q: &mut Queue<QueueItem>, fresh: &[crate::library::Resolved]) -> usize {
    q.update_others(|i| {
        let Some(r) = i.track_id.and_then(|t| fresh.iter().find(|r| r.id == t)) else {
            return false;
        };
        let item = QueueItem {
            path: r.path.clone(),
            title: r.title.clone(),
            artists: (!r.artists.is_empty()).then(|| r.artists.clone()),
            track_id: i.track_id,
            cover: r.cover.clone(),
            art: r.art,
            source: i.source,
            album_key: r.album_id.map_or(0, |v| v as u64),
            artist_key: r.artist_id.map_or(0, |v| v as u64),
        };
        if *i == item {
            false
        } else {
            *i = item;
            true
        }
    })
}

/// Library tracks as queue items.
pub(crate) fn items_for(
    lib: &crate::library::Library,
    ids: &[u32],
    context: PlayContext,
) -> Result<Vec<QueueItem>, AppError> {
    items_with_source(lib, ids, context.into())
}

/// Library tracks as queue items stamped with their source (`play_event.source`).
pub(crate) fn items_with_source(
    lib: &crate::library::Library,
    ids: &[u32],
    source: PlaySource,
) -> Result<Vec<QueueItem>, AppError> {
    Ok(lib
        .resolve(ids)?
        .into_iter()
        .map(|r| QueueItem {
            path: r.path,
            title: r.title,
            artists: (!r.artists.is_empty()).then_some(r.artists),
            track_id: Some(r.id),
            cover: r.cover,
            art: r.art,
            source,
            album_key: r.album_id.map_or(0, |v| v as u64),
            artist_key: r.artist_id.map_or(0, |v| v as u64),
        })
        .collect())
}

/// Plays library tracks `ids` as one context starting at `start` (double-click in a list).
#[tauri::command(async)]
#[specta::specta]
pub fn play_tracks(
    app: AppHandle,
    pb: State<'_, Playback>,
    lib: State<'_, crate::library::Library>,
    ids: Vec<u32>,
    start: u32,
    context: PlayContext,
    playlist: Option<u32>,
) -> Result<(), AppError> {
    use crate::continuation::Origin;
    pb.play_queue(items_for(&lib, &ids, context)?, start as usize);
    // ∞ (T3.14): what the queue continues from.
    let origin = match (context, playlist) {
        (PlayContext::Wave, _) => Origin::None,
        (PlayContext::Playlist, Some(id)) => Origin::Playlist(i64::from(id)),
        (PlayContext::Playlist, None) => Origin::None,
        _ => Origin::List,
    };
    if origin != Origin::None {
        let cont = app.state::<crate::continuation::Continuation>();
        cont.start(&app, origin, ids.iter().map(|&i| i64::from(i)).collect());
    }
    Ok(())
}

/// «Воспроизвести далее» (FR-PLAY-006).
#[tauri::command(async)]
#[specta::specta]
pub fn queue_play_next(
    pb: State<'_, Playback>,
    lib: State<'_, crate::library::Library>,
    ids: Vec<u32>,
    context: PlayContext,
) -> Result<(), AppError> {
    pb.play_next(items_for(&lib, &ids, context)?);
    Ok(())
}

/// «В конец очереди».
#[tauri::command(async)]
#[specta::specta]
pub fn queue_append(
    pb: State<'_, Playback>,
    lib: State<'_, crate::library::Library>,
    ids: Vec<u32>,
    context: PlayContext,
) -> Result<(), AppError> {
    pb.append(items_for(&lib, &ids, context)?);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn queue_get(
    app: AppHandle,
    pb: State<'_, Playback>,
    cont: State<'_, crate::continuation::Continuation>,
) -> QueueView {
    let mut v = pb.queue_view();
    v.autoplay = cont.enabled(&app);
    if let crate::continuation::Origin::Playlist(id) = cont.origin() {
        v.playlist = u32::try_from(id).ok();
    }
    v
}

#[tauri::command]
#[specta::specta]
pub fn queue_remove(pb: State<'_, Playback>, index: u32) {
    pb.queue_remove(index as usize);
}

#[tauri::command]
#[specta::specta]
pub fn queue_move(pb: State<'_, Playback>, from: u32, to: u32) {
    pb.queue_move(from as usize, to as usize);
}

#[tauri::command]
#[specta::specta]
pub fn queue_jump(pb: State<'_, Playback>, index: u32) {
    pb.queue_jump(index as usize);
}

#[tauri::command]
#[specta::specta]
pub fn playback_set_repeat(pb: State<'_, Playback>, mode: RepeatMode) {
    pb.set_repeat(mode.into());
}

#[tauri::command(async)]
#[specta::specta]
pub fn playback_set_shuffle(
    pb: State<'_, Playback>,
    lib: State<'_, crate::library::Library>,
    on: bool,
) {
    let day_ago = sirtone_library::db::now_ms() - 24 * 3600 * 1000;
    pb.set_shuffle(on, &lib.recent_track_ids(day_ago));
}
