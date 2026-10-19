//! Playlist continuation ∞ (T3.14, FR-PLS-003/005, docs/04 §11; `01b` F-14 — Roon Radio,
//! Apple Music ∞, Spotify Smart Shuffle «+»/«−»). After a playlist (or an album, artist,
//! collection or search list, if ∞ is on for those) the queue goes on with similar tracks from
//! the library only, closest first; when nothing close enough is left, the queue simply ends and
//! playback stops. The playlist page shows the same sequence under «Рекомендовано из вашей
//! медиатеки». Ranking runs on a worker thread; the playback sink only asks.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use sirtone_core::error::{AppError, ErrorCode};
use sirtone_core::wave::PlaySource;
use sirtone_library::continuation as store;
use sirtone_wave::similar::{SimConfig, Similar};
use tauri::{AppHandle, Manager, State};

use crate::library::{Library, TrackItem};
use crate::playback::{Playback, items_with_source};
use crate::settings_store::SettingsStore;

/// Tracks added per top-up (the sink asks again with fewer than two ahead).
const MORE: usize = 2;
/// Tracks the playlist page previews.
const PREVIEW: usize = 20;
/// Played within this window — not offered (the wave's repeat window, docs/14 §9).
const RECENT_MS: i64 = 24 * 3600 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Origin {
    /// Nothing to continue (the wave, files from Explorer, empty).
    #[default]
    None,
    Playlist(i64),
    /// An album, artist, collection or search list (`playback.autoplay_albums`).
    List,
}

#[derive(Default)]
struct Session {
    origin: Origin,
    seeds: Vec<i64>,
    /// Recommendations queued so far, in order: the sequence goes on from them.
    picked: Vec<i64>,
    /// «−» without a playlist: for this session only.
    minus: HashSet<i64>,
    exhausted: bool,
    /// Bumped on every new session: a late result of an old one is dropped.
    epoch: u64,
}

#[derive(Default)]
pub struct Continuation {
    session: Mutex<Session>,
    /// ∞ is on for this session and something may still be close enough.
    active: AtomicBool,
    busy: AtomicBool,
    /// The prepared library, keyed by what invalidates it (tracks and their analysis).
    cache: Mutex<Option<(String, Arc<Similar>)>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_ms() -> i64 {
    sirtone_library::db::now_ms()
}

impl Continuation {
    /// The sink's cheap check: worth asking for more?
    pub fn active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// A new context starts playing (replaces the session).
    pub fn start(&self, app: &AppHandle, origin: Origin, seeds: Vec<i64>) {
        // A playlist continues from what it holds (not from a list the page passed); a smart
        // playlist has no ∞ (D-86).
        let (origin, seeds) = match origin {
            Origin::Playlist(id) => {
                let lib = app.state::<Library>();
                if lib.with_read(|c| store::manual(c, id)).unwrap_or(false) {
                    (
                        origin,
                        lib.with_read(|c| store::seeds(c, id)).unwrap_or(seeds),
                    )
                } else {
                    (Origin::None, Vec::new())
                }
            }
            _ => (origin, seeds),
        };
        {
            let mut s = lock(&self.session);
            let epoch = s.epoch + 1;
            *s = Session {
                origin,
                seeds,
                epoch,
                ..Session::default()
            };
        }
        self.refresh(app);
    }

    /// Something without a continuation took over the queue.
    pub fn reset(&self) {
        let mut s = lock(&self.session);
        s.origin = Origin::None;
        s.epoch += 1;
        self.active.store(false, Ordering::Relaxed);
    }

    pub fn origin(&self) -> Origin {
        lock(&self.session).origin
    }

    /// ∞ for the current session: `None` when it does not apply.
    pub fn enabled(&self, app: &AppHandle) -> Option<bool> {
        match self.origin() {
            Origin::None => None,
            Origin::Playlist(id) => Some(
                app.state::<Library>()
                    .with_read(|c| store::hybrid(c, id))
                    .unwrap_or(false),
            ),
            Origin::List => Some(
                app.try_state::<Arc<SettingsStore>>()
                    .and_then(|s| s.get()["playback"]["autoplay_albums"].as_bool())
                    .unwrap_or(false),
            ),
        }
    }

    fn refresh(&self, app: &AppHandle) {
        let on = self.enabled(app) == Some(true) && !lock(&self.session).exhausted;
        self.active.store(on, Ordering::Relaxed);
    }

    /// The library prepared for ranking; rebuilt (outside the library lock) when tracks or
    /// their analysis changed.
    fn similar(&self, lib: &Library) -> Result<Arc<Similar>, AppError> {
        let key: String = lib.with_read(|c| {
            Ok(c.query_row(
                "SELECT count(*) || ':' || coalesce(max(id), 0) || ':' ||
                        coalesce(max(analyzed_at), 0) FROM track WHERE state = 'ok'",
                [],
                |r| r.get(0),
            )?)
        })?;
        if let Some((k, s)) = lock(&self.cache).as_ref()
            && *k == key
        {
            return Ok(s.clone());
        }
        let cfg =
            SimConfig::builtin().map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))?;
        let tracks = lib.with_read(store::sim_tracks)?;
        let s = Arc::new(Similar::build(&tracks, cfg));
        *lock(&self.cache) = Some((key, s.clone()));
        Ok(s)
    }

    /// The sink saw fewer than two tracks ahead: rank and append on a worker thread.
    pub fn request_more(&self, app: &AppHandle) {
        if !self.active() || self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("sirtone-continuation".into())
            .spawn(move || {
                let me = app.state::<Continuation>();
                if let Err(e) = me.top_up(&app) {
                    tracing::warn!(target: "sirtone::continuation", code = ?e.code, "top-up failed");
                    me.active.store(false, Ordering::Relaxed);
                }
                me.busy.store(false, Ordering::Release);
            });
        if spawned.is_err() {
            self.busy.store(false, Ordering::Release);
        }
    }

    fn top_up(&self, app: &AppHandle) -> Result<(), AppError> {
        let (origin, seeds, picked, minus, epoch) = {
            let s = lock(&self.session);
            (
                s.origin,
                s.seeds.clone(),
                s.picked.clone(),
                s.minus.clone(),
                s.epoch,
            )
        };
        let playlist = match origin {
            Origin::None => return Ok(()),
            Origin::Playlist(id) => Some(id),
            Origin::List => None,
        };
        let lib = app.state::<Library>();
        let sim = self.similar(&lib)?;
        let mut blocked = lib.with_read(|c| store::blocked(c, playlist, now_ms() - RECENT_MS))?;
        blocked.extend(minus);
        let picks = sim.continuation(&seeds, &blocked, &picked, MORE);
        let ids: Vec<u32> = picks.iter().map(|p| p.id as u32).collect();
        let items = if ids.is_empty() {
            Vec::new()
        } else {
            items_with_source(&lib, &ids, PlaySource::Hybrid)?
        };
        {
            let mut s = lock(&self.session);
            if s.epoch != epoch {
                return Ok(());
            }
            if items.is_empty() {
                // Nothing close enough: the queue ends here and playback stops (FR-PLS-003).
                s.exhausted = true;
                self.active.store(false, Ordering::Relaxed);
                return Ok(());
            }
            s.picked.extend(picks.iter().map(|p| p.id));
        }
        app.state::<Playback>().append(items);
        Ok(())
    }

    /// The playlist page: what ∞ would play after it (the same sequence as the queue).
    pub fn preview(&self, lib: &Library, playlist: i64) -> Result<Vec<i64>, AppError> {
        let sim = self.similar(lib)?;
        let (seeds, blocked) = lib.with_read(|c| {
            Ok((
                store::seeds(c, playlist)?,
                store::blocked(c, Some(playlist), now_ms() - RECENT_MS)?,
            ))
        })?;
        Ok(sim
            .continuation(&seeds, &blocked, &[], PREVIEW)
            .into_iter()
            .map(|p| p.id)
            .collect())
    }

    /// ∞ switched (queue panel or playlist page).
    pub fn set_enabled(
        &self,
        app: &AppHandle,
        playlist: Option<i64>,
        on: bool,
    ) -> Result<(), AppError> {
        match playlist {
            Some(id) => app
                .state::<Library>()
                .with_write(|c| store::set_hybrid(c, id, on))?,
            None => {
                if let Some(s) = app.try_state::<Arc<SettingsStore>>() {
                    s.patch(&serde_json::json!({ "playback": { "autoplay_albums": on } }))
                        .map_err(|e| AppError::new(ErrorCode::InvalidArgument).with_details(e))?;
                }
            }
        }
        let ours = match self.origin() {
            Origin::Playlist(id) => playlist == Some(id),
            Origin::List => playlist.is_none(),
            Origin::None => false,
        };
        if ours {
            lock(&self.session).exhausted = false;
            self.refresh(app);
            if !on {
                // Apple Music ∞ off: the queued recommendations go too; the one playing stays
                // a pick (not offered again when ∞ comes back on).
                let pb = app.state::<Playback>();
                pb.drop_upcoming(|i| i.source == PlaySource::Hybrid);
                lock(&self.session).picked = pb.played_hybrid();
            }
            app.state::<Playback>().changed();
        }
        Ok(())
    }

    /// «−» on a recommended track: never for this playlist again (or this session, without
    /// one); it leaves the upcoming queue and the sequence is picked again from there.
    pub fn exclude(
        &self,
        app: &AppHandle,
        playlist: Option<i64>,
        track: u32,
    ) -> Result<(), AppError> {
        if let Some(id) = playlist {
            app.state::<Library>()
                .with_write(|c| store::exclude(c, id, i64::from(track)))?;
        }
        let ours = {
            let mut s = lock(&self.session);
            let ours = match s.origin {
                Origin::Playlist(id) => playlist.is_none_or(|p| p == id),
                Origin::List => playlist.is_none(),
                Origin::None => false,
            };
            if ours {
                s.minus.insert(i64::from(track));
                s.exhausted = false;
            }
            ours
        };
        if ours {
            // The queued picks are ranked again without it, from what already played.
            let pb = app.state::<Playback>();
            pb.drop_upcoming(|i| i.source == PlaySource::Hybrid);
            lock(&self.session).picked = pb.played_hybrid();
            self.refresh(app);
        }
        Ok(())
    }

    /// Tracks were added to a playlist: if it is the one playing, the new ones become part of
    /// it (seeds), and queued recommendations of them are no longer «recommended» («+»).
    pub fn playlist_changed(&self, app: &AppHandle, playlist: i64, added: &[u32]) {
        if self.origin() != Origin::Playlist(playlist) {
            return;
        }
        let seeds = app
            .state::<Library>()
            .with_read(|c| store::seeds(c, playlist))
            .unwrap_or_default();
        {
            let mut s = lock(&self.session);
            s.seeds = seeds;
            s.exhausted = false;
        }
        if !added.is_empty() {
            app.state::<Playback>().promote(added);
        }
        self.refresh(app);
    }
}

/// The recommendations under a playlist (FR-PLS-003): empty with ∞ off or nothing close.
#[tauri::command(async)]
#[specta::specta]
pub fn continuation_preview(
    lib: State<'_, Library>,
    cont: State<'_, Continuation>,
    playlist: u32,
) -> Result<Vec<TrackItem>, AppError> {
    let id = i64::from(playlist);
    if !lib.with_read(|c| store::hybrid(c, id))? {
        return Ok(Vec::new());
    }
    let ids: Vec<u32> = cont
        .preview(&lib, id)?
        .into_iter()
        .map(|i| i as u32)
        .collect();
    lib.track_items(&ids)
}

/// ∞ on/off: a playlist's own, or (no playlist) the one for albums and other lists.
#[tauri::command(async)]
#[specta::specta]
pub fn continuation_set(
    app: AppHandle,
    cont: State<'_, Continuation>,
    playlist: Option<u32>,
    on: bool,
) -> Result<(), AppError> {
    cont.set_enabled(&app, playlist.map(i64::from), on)?;
    if let Some(id) = playlist {
        crate::playlists::changed(&app, id);
    }
    Ok(())
}

/// «−» on a recommended track.
#[tauri::command(async)]
#[specta::specta]
pub fn continuation_exclude(
    app: AppHandle,
    cont: State<'_, Continuation>,
    playlist: Option<u32>,
    track: u32,
) -> Result<(), AppError> {
    cont.exclude(&app, playlist.map(i64::from), track)?;
    if let Some(id) = playlist {
        crate::playlists::changed(&app, id);
    }
    Ok(())
}
