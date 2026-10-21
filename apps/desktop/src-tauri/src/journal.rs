//! Listening journal (T1.31, FR-LEARN-001): one `play_event` per heard track with the context
//! at its start. The tracker follows engine snapshots and user actions; rows go to a writer
//! thread (the engine thread never waits for the disk), one transaction each, so a crash loses
//! at most the event in flight. Library tracks only — external files have no `track_id`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sirtone_audio::{PlayState, Snapshot};
use sirtone_core::wave::{Clock, ContextSnapshot, EndReason, PlayEvent, PlaySource, WaveRepo};
use sirtone_library::wave::SqliteWaveRepo;
use sirtone_platform::clock::SystemClock;
use sirtone_wave::context::ContextBuilder;

use crate::settings_store::SettingsStore;

/// The track being heard right now.
struct Open {
    path: PathBuf,
    track_id: i64,
    source: PlaySource,
    ts: i64,
    duration_ms: u64,
    heard: Duration,
    playing_since: Option<Instant>,
    context: ContextSnapshot,
    /// The wave profile the play came from (`my_wave`, `seasonal`), for wave plays.
    profile: Option<String>,
}

impl Open {
    fn heard_ms(&self) -> u64 {
        let live = self.playing_since.map_or(Duration::ZERO, |t| t.elapsed());
        (self.heard + live).as_millis() as u64
    }
}

/// What a snapshot is about: the queue entry for the snapshot's path, if it is a library track.
pub struct Item<'a> {
    pub path: &'a Path,
    pub track_id: Option<u32>,
    pub source: PlaySource,
}

pub struct Journal {
    open: Mutex<Option<Open>>,
    tx: Mutex<Option<Sender<PlayEvent>>>,
    writer: Mutex<Option<std::thread::JoinHandle<()>>>,
    clock: Arc<dyn Clock>,
    /// `None` — the app's shared context (`context::builder`); tests pin one.
    context: Option<ContextBuilder>,
    /// The running wave's profile, stamped on wave plays as they open.
    wave_profile: Mutex<Option<String>>,
}

impl Journal {
    pub fn start(settings: Arc<SettingsStore>) -> Self {
        Self::with(Arc::new(SystemClock), move || {
            settings.get()["library"]["root"]
                .as_str()
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        })
    }

    pub fn with(
        clock: Arc<dyn Clock>,
        root: impl Fn() -> Option<PathBuf> + Send + 'static,
    ) -> Self {
        let (tx, rx) = channel();
        let writer = std::thread::Builder::new()
            .name("sirtone-journal".into())
            .spawn(move || write_loop(&rx, &root))
            .ok();
        Self {
            open: Mutex::new(None),
            tx: Mutex::new(Some(tx)),
            writer: Mutex::new(writer),
            clock,
            // The app's shared context (city, weather) at each snapshot.
            context: None,
            wave_profile: Mutex::new(None),
        }
    }

    /// The running wave's profile (`my_wave`, `seasonal`), if a wave was started.
    pub fn wave_profile(&self) -> Option<String> {
        self.wave_profile
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// The wave started with this profile (learning knows where its plays came from, §8.4).
    pub fn set_wave_profile(&self, id: &str) {
        *self.wave_profile.lock().unwrap_or_else(|p| p.into_inner()) = Some(id.to_owned());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Open>> {
        self.open.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn send(&self, open: Open, reason: EndReason) {
        // An event wave's plays name their event (T5.13).
        let event_id = (open.profile.as_deref() == Some("event"))
            .then(crate::events::accepted)
            .flatten();
        let event = PlayEvent {
            ts: open.ts,
            track_id: open.track_id,
            played_ms: open.heard_ms(),
            duration_ms: open.duration_ms,
            end_reason: reason,
            source: open.source,
            profile: open.profile,
            context: open.context,
            event_id,
            novelty: None,
            category: None,
        };
        if let Some(tx) = self.tx.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            let _ = tx.send(event);
        }
    }

    /// Closes the current play with `reason` (user pressed next/prev, started another context,
    /// quit). Idempotent: a closed play is never written twice.
    pub fn close(&self, reason: EndReason) {
        let open = self.lock().take();
        if let Some(open) = open {
            self.send(open, reason);
        }
    }

    /// Follows the engine: a new path means the previous track was heard to its end (gapless
    /// switch); `Ended` closes as finished; the first `Playing` of a library track opens a play.
    pub fn observe(&self, s: &Snapshot, item: Option<Item<'_>>) {
        let mut guard = self.lock();
        if let Some(open) = guard.as_mut() {
            if s.path.as_deref() != Some(open.path.as_path()) {
                if let Some(open) = guard.take() {
                    drop(guard);
                    // An automix overlap cut it short on purpose: heard to the end (T4.9).
                    let reason = if s.mixing {
                        EndReason::MixedOut
                    } else {
                        EndReason::Finished
                    };
                    self.send(open, reason);
                    guard = self.lock();
                }
            } else if s.state == PlayState::Ended {
                if let Some(open) = guard.take() {
                    drop(guard);
                    self.send(open, EndReason::Finished);
                }
                return;
            } else {
                match (s.state == PlayState::Playing, open.playing_since) {
                    (true, None) => open.playing_since = Some(Instant::now()),
                    (false, Some(t)) => {
                        open.heard += t.elapsed();
                        open.playing_since = None;
                    }
                    _ => {}
                }
                if let Some(d) = s.duration_ms {
                    open.duration_ms = d;
                }
                return;
            }
        }
        if guard.is_none()
            && s.state == PlayState::Playing
            && let Some(item) = item
            && s.path.as_deref() == Some(item.path)
            && let Some(id) = item.track_id
        {
            let now = self.clock.now_utc_ms();
            *guard = Some(Open {
                path: item.path.to_path_buf(),
                track_id: i64::from(id),
                source: item.source,
                ts: now,
                duration_ms: s.duration_ms.unwrap_or(0),
                heard: Duration::ZERO,
                playing_since: Some(Instant::now()),
                context: self
                    .context
                    .as_ref()
                    .map_or_else(crate::context::builder, Clone::clone)
                    .snapshot_at(self.clock.as_ref(), now),
                profile: (item.source == PlaySource::Wave)
                    .then(|| {
                        self.wave_profile
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .clone()
                    })
                    .flatten(),
            });
        }
    }

    /// The engine could not play `path`: if that is the current track, its play ends in error.
    pub fn on_error(&self, path: Option<&Path>) {
        let mut guard = self.lock();
        if guard
            .as_ref()
            .is_some_and(|o| Some(o.path.as_path()) == path)
            && let Some(open) = guard.take()
        {
            drop(guard);
            self.send(open, EndReason::Error);
        }
    }

    /// Normal exit: the current play ends as `quit`, then waits (≤ 2 s) for the writer.
    pub fn shutdown(&self) {
        self.close(EndReason::Quit);
        drop(self.tx.lock().unwrap_or_else(|p| p.into_inner()).take());
        if let Some(h) = self.writer.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !h.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if h.is_finished() {
                let _ = h.join();
            }
        }
    }
}

fn write_loop(rx: &Receiver<PlayEvent>, root: &dyn Fn() -> Option<PathBuf>) {
    let mut repo: Option<(PathBuf, SqliteWaveRepo)> = None;
    while let Ok(event) = rx.recv() {
        let Some(root) = root() else { continue };
        if repo.as_ref().is_none_or(|(r, _)| *r != root) {
            let db = crate::library::db_path(&root);
            if !db.exists() {
                continue; // no library yet: nothing to attach the event to
            }
            repo = SqliteWaveRepo::open(&db, crate::library::medium(&root))
                .ok()
                .map(|r| (root.clone(), r));
        }
        let failed = match repo.as_ref() {
            Some((_, r)) => r.record_play(&event).err(),
            None => None,
        };
        if let Some(e) = failed {
            // A track removed meanwhile (foreign key) or the library went offline: the event is
            // dropped and the connection reopened next time (a returned stick is a new file).
            tracing::warn!(target: "sirtone::journal", code = ?e.code, "play event not recorded");
            repo = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sirtone_wave::clock::FixedClock;

    fn snap(state: PlayState, path: &str) -> Snapshot {
        Snapshot {
            state,
            path: Some(PathBuf::from(path)),
            position_ms: 0,
            duration_ms: Some(180_000),
            volume: 1.0,
            underruns: 0,
            device: None,
            signal: None,
            mixing: false,
        }
    }

    fn item(path: &str, id: u32) -> Item<'_> {
        Item {
            path: Path::new(path),
            track_id: Some(id),
            source: PlaySource::Album,
        }
    }

    /// Journal without a writer: events land in a channel the test reads.
    fn journal() -> (Journal, Receiver<PlayEvent>) {
        let (tx, rx) = channel();
        let j = Journal {
            open: Mutex::new(None),
            tx: Mutex::new(Some(tx)),
            writer: Mutex::new(None),
            clock: Arc::new(FixedClock::local(2026, 11, 22, 19, 0, 180)),
            context: Some(ContextBuilder::default()),
            wave_profile: Mutex::new(None),
        };
        (j, rx)
    }

    /// T4.9: an automix overlap ends A as «mixed out» (a complete listen), not a skip.
    #[test]
    fn an_automix_handover_is_mixed_out() {
        let (j, rx) = journal();
        j.observe(&snap(PlayState::Playing, "a"), Some(item("a", 1)));
        let mut b = snap(PlayState::Playing, "b");
        b.mixing = true;
        j.observe(&b, Some(item("b", 2)));
        assert_eq!(rx.try_recv().unwrap().end_reason, EndReason::MixedOut);
    }

    #[test]
    fn natural_end_gapless_skip_and_quit() {
        let (j, rx) = journal();
        j.observe(&snap(PlayState::Playing, "a"), Some(item("a", 1)));
        j.observe(&snap(PlayState::Playing, "a"), Some(item("a", 1)));
        // gapless: the engine is already on b
        j.observe(&snap(PlayState::Playing, "b"), Some(item("b", 2)));
        let a = rx.try_recv().unwrap();
        assert_eq!((a.track_id, a.end_reason), (1, EndReason::Finished));
        assert_eq!(a.source, PlaySource::Album);
        assert_eq!(a.context.weekday, 6);
        // b ends naturally; repeated Ended snapshots write once
        j.observe(&snap(PlayState::Ended, "b"), Some(item("b", 2)));
        j.observe(&snap(PlayState::Ended, "b"), Some(item("b", 2)));
        assert_eq!(rx.try_recv().unwrap().end_reason, EndReason::Finished);
        assert!(rx.try_recv().is_err());
        // c, the user presses next → skip; the late snapshot of c does not reopen it twice
        j.observe(&snap(PlayState::Playing, "c"), Some(item("c", 3)));
        j.close(EndReason::Skip);
        j.close(EndReason::Skip);
        assert_eq!(rx.try_recv().unwrap().end_reason, EndReason::Skip);
        assert!(rx.try_recv().is_err());
        j.observe(&snap(PlayState::Playing, "d"), Some(item("d", 4)));
        j.shutdown();
        assert_eq!(rx.try_recv().unwrap().end_reason, EndReason::Quit);
    }

    #[test]
    fn wave_plays_carry_the_profile() {
        let (j, rx) = journal();
        j.set_wave_profile("seasonal");
        let wave = |p: &'static str, id| Item {
            path: Path::new(p),
            track_id: Some(id),
            source: PlaySource::Wave,
        };
        j.observe(&snap(PlayState::Playing, "w"), Some(wave("w", 5)));
        // A new wave starts: the open play keeps the profile it began with.
        j.set_wave_profile("my_wave");
        j.observe(&snap(PlayState::Playing, "a"), Some(item("a", 1)));
        let w = rx.try_recv().unwrap();
        assert_eq!(w.profile.as_deref(), Some("seasonal"));
        j.close(EndReason::Skip);
        assert_eq!(rx.try_recv().unwrap().profile, None, "not a wave play");
    }

    #[test]
    fn paused_time_is_not_heard_and_externals_are_skipped() {
        let (j, rx) = journal();
        j.observe(
            &snap(PlayState::Playing, "x.mp3"),
            Some(Item {
                path: Path::new("x.mp3"),
                track_id: None,
                source: PlaySource::External,
            }),
        );
        j.close(EndReason::Skip);
        assert!(rx.try_recv().is_err(), "external files are not journaled");
        j.observe(&snap(PlayState::Playing, "a"), Some(item("a", 1)));
        std::thread::sleep(Duration::from_millis(60));
        j.observe(&snap(PlayState::Paused, "a"), Some(item("a", 1)));
        std::thread::sleep(Duration::from_millis(200));
        j.on_error(Some(Path::new("other")));
        j.on_error(Some(Path::new("a")));
        let e = rx.try_recv().unwrap();
        assert_eq!(e.end_reason, EndReason::Error);
        assert!((50..190).contains(&e.played_ms), "{}", e.played_ms);
    }
}
