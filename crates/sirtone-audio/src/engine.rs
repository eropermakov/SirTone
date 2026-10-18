//! Engine thread (docs/04 §1): commands in, decode → channel map → resample to the device rate →
//! ring; snapshots and events out. Heavy work lives here, never in the render thread.
//!
//! Prototype scope (0.2): one deck, shared mode, default device. Gapless, automix, EQ, normaliser
//! and the visualiser tap come with T1.5+ and Stage 4.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use sirtone_core::error::{AppError, ErrorCode};

use crate::decode::Source;
use crate::output::{DeviceFormat, Output, Shared};

#[derive(Debug, Clone)]
pub enum EngineCmd {
    Load {
        path: PathBuf,
        autoplay: bool,
    },
    Play,
    Pause,
    Toggle,
    /// Position in milliseconds.
    Seek(u64),
    /// Slider value 0…1; the engine applies the cubic curve (docs/04 §7).
    SetVolume(f32),
    /// The track to continue with when the current one ends, without a gap (T1.5, docs/04 §3:
    /// pre-opened and fed into the same ring). `None` clears it.
    SetNext(Option<PathBuf>),
    /// FR-PLAY-012: pause when the output device disappears (default on), else follow the new default.
    SetPauseOnDeviceLoss(bool),
    /// From the device watcher: the system default output changed.
    DefaultDeviceChanged,
    /// From the device watcher: an endpoint was unplugged, disabled or removed.
    DeviceLost(String),
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayState {
    Empty,
    Playing,
    Paused,
    Ended,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub state: PlayState,
    pub path: Option<PathBuf>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub underruns: u64,
    /// Endpoint id of the current output device.
    pub device: Option<String>,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    Snapshot(Snapshot),
    Error {
        error: AppError,
        path: Option<PathBuf>,
    },
}

pub type EventSink = Box<dyn Fn(EngineEvent) + Send>;

/// Cheap clonable handle; dropping the last one does not stop the engine — send `Shutdown`.
#[derive(Clone)]
pub struct EngineHandle {
    tx: Sender<EngineCmd>,
}

impl EngineHandle {
    pub fn send(&self, cmd: EngineCmd) {
        let _ = self.tx.send(cmd);
    }
}

/// Starts the engine thread. The output device is opened lazily on the first `Load`.
pub fn start(events: EventSink) -> Result<EngineHandle, AppError> {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher_stop = stop.clone();
    std::thread::Builder::new()
        .name("sirtone-engine".into())
        .spawn(move || {
            Engine::new(events).run(rx);
            stop.store(true, Ordering::Relaxed);
        })
        .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))?;
    let handle = EngineHandle { tx };
    crate::devices::watch(handle.clone(), watcher_stop);
    Ok(handle)
}

/// Windows sends «default changed» and «device gone» in either order when headphones are pulled:
/// decide after this delay whether the old device is still there (follow) or gone (pause).
const DEVICE_SETTLE: Duration = Duration::from_millis(250);

const CHUNK: usize = 1024;
const SNAPSHOT_EVERY: Duration = Duration::from_millis(250);

pub(crate) struct Deck {
    pub(crate) source: Source,
    path: PathBuf,
    resampler: Option<Fft<f32>>,
    /// Device-rate output frames still to drop (resampler delay after reset).
    skip_out: usize,
    /// Channel-mapped, not yet resampled input (interleaved, device channel count).
    pending: Vec<f32>,
    decoded: Vec<f32>,
    out_buf: Vec<f32>,
    pub(crate) eof: bool,
    /// Input frames fed since the segment start (to bound the resampled tail).
    in_frames: u64,
    /// Zero-padded chunks fed after the decoder ended (flushes the resampler delay).
    tail_chunks: u32,
    /// Device-rate frames pushed since the segment start.
    pushed: u64,
    /// Segment start: track position (device frames) and `Shared::consumed` at that moment.
    base_frames: u64,
    base_consumed: u64,
    /// Length of the track that is audible now (the source may already be the next one).
    duration_ms: Option<u64>,
    /// Source frames decoded since the deck start.
    decoded_in: u64,
    /// Gapless chain (T1.5): the next track with the same rate and channels continues through the
    /// same resampler, so the joint is exactly as continuous as one file would be.
    pub(crate) chained: Option<(Source, PathBuf)>,
    /// Where chained tracks begin, in deck frames (output and input), oldest first.
    boundaries: VecDeque<Boundary>,
}

struct Boundary {
    at_out: u64,
    at_in: u64,
    path: PathBuf,
    duration_ms: Option<u64>,
}

struct Engine {
    events: EventSink,
    shared: Arc<Shared>,
    output: Option<(Output, rtrb::Producer<f32>)>,
    deck: Option<Deck>,
    state: PlayState,
    volume: f32,
    last_snapshot: Instant,
    flush_seq: u64,
    lost_position: Option<u64>,
    /// Gapless continuation: path from `SetNext`, the deck being fed after the current one ended
    /// (it becomes audible at its `base_consumed`), and whether opening it already failed.
    next_path: Option<PathBuf>,
    feeding: Option<Deck>,
    next_failed: bool,
    /// The next track is attached to the current deck's chain (same format).
    next_attached: bool,
    /// The next track in another format, opened early (docs/04 §3) and used at the joint.
    next_source: Option<(Source, PathBuf)>,
    pause_on_device_loss: bool,
    /// A default-device change waiting to settle (see `DEVICE_SETTLE`).
    default_changed_at: Option<Instant>,
    /// Underruns already reported in the log (diagnostics; the RT thread only counts).
    logged_underruns: u64,
}

impl Engine {
    fn new(events: EventSink) -> Self {
        Self {
            events,
            shared: Shared::new(),
            output: None,
            deck: None,
            state: PlayState::Empty,
            volume: 1.0,
            last_snapshot: Instant::now(),
            flush_seq: 0,
            lost_position: None,
            next_path: None,
            feeding: None,
            next_failed: false,
            next_attached: false,
            next_source: None,
            pause_on_device_loss: true,
            default_changed_at: None,
            logged_underruns: 0,
        }
    }

    fn run(mut self, rx: mpsc::Receiver<EngineCmd>) {
        loop {
            let busy = self.state == PlayState::Playing
                && (self.deck.as_ref().is_some_and(|d| !d.eof)
                    || self.feeding.as_ref().is_some_and(|d| !d.eof));
            let wait = if busy {
                Duration::from_millis(5)
            } else {
                Duration::from_millis(50)
            };
            match rx.recv_timeout(wait) {
                Ok(EngineCmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(cmd) => self.handle(cmd),
                Err(RecvTimeoutError::Timeout) => {}
            }
            if self.shared.failed.load(Ordering::Acquire) {
                self.on_device_lost();
            }
            // Every underrun lands in the log with where it happened (owner-reported choppy
            // sound must never be a guess again).
            let u = self.shared.underruns.load(Ordering::Relaxed);
            if u > self.logged_underruns {
                tracing::warn!(
                    target: "sirtone::audio",
                    underruns = u,
                    new = u - self.logged_underruns,
                    position_ms = self.position_ms(),
                    feeding_next = self.feeding.is_some(),
                    "output underrun"
                );
                self.logged_underruns = u;
            }
            if self
                .default_changed_at
                .is_some_and(|t| t.elapsed() >= DEVICE_SETTLE)
            {
                self.default_changed_at = None;
                self.on_default_changed();
            }
            if let Err(e) = self.pump() {
                self.fail(e);
            }
            self.check_end();
            if self.state == PlayState::Playing && self.last_snapshot.elapsed() >= SNAPSHOT_EVERY {
                self.emit_snapshot();
            }
        }
        self.output = None;
    }

    fn handle(&mut self, cmd: EngineCmd) {
        match cmd {
            EngineCmd::Load { path, autoplay } => {
                if let Err(e) = self.load(path.clone(), autoplay) {
                    (self.events)(EngineEvent::Error {
                        error: e,
                        path: Some(path),
                    });
                }
            }
            EngineCmd::Play => self.set_playing(true),
            EngineCmd::Pause => self.set_playing(false),
            EngineCmd::Toggle => self.set_playing(self.state != PlayState::Playing),
            EngineCmd::Seek(ms) => {
                if let Err(e) = self.seek(ms) {
                    self.fail(e);
                }
            }
            EngineCmd::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0);
                self.shared
                    .gain_bits
                    .store(self.volume.powi(3).to_bits(), Ordering::Relaxed);
            }
            EngineCmd::SetNext(path) => {
                if self.next_path != path {
                    self.next_path = path;
                    self.feeding = None;
                    self.next_failed = false;
                    self.next_attached = false;
                    self.next_source = None;
                    if let Some(d) = self.deck.as_mut() {
                        d.chained = None;
                    }
                }
                return;
            }
            EngineCmd::SetPauseOnDeviceLoss(on) => {
                self.pause_on_device_loss = on;
                return;
            }
            EngineCmd::DefaultDeviceChanged => {
                self.default_changed_at = Some(Instant::now());
                return;
            }
            EngineCmd::DeviceLost(id) => {
                if self.output.as_ref().is_some_and(|(o, _)| o.device_id == id) {
                    self.on_output_gone();
                }
                return;
            }
            EngineCmd::Shutdown => {}
        }
        self.emit_snapshot();
    }

    fn ensure_output(&mut self) -> Result<DeviceFormat, AppError> {
        if let Some((out, _)) = &self.output {
            return Ok(out.format);
        }
        let (out, prod) = Output::open(self.shared.clone())?;
        let f = out.format;
        self.output = Some((out, prod));
        Ok(f)
    }

    fn load(&mut self, path: PathBuf, autoplay: bool) -> Result<(), AppError> {
        let source = Source::open(&path)?;
        let fmt = self.ensure_output()?;
        self.flush();
        self.lost_position = None;
        let mut deck = new_deck(source, path, fmt, 0)?;
        deck.base_consumed = self.shared.consumed.load(Ordering::Relaxed);
        self.deck = Some(deck);
        self.feeding = None;
        self.next_failed = false;
        self.next_attached = false;
        self.next_source = None;
        tracing::info!(target: "sirtone::audio", "track loaded");
        self.state = if autoplay {
            PlayState::Playing
        } else {
            PlayState::Paused
        };
        self.shared.playing.store(autoplay, Ordering::Relaxed);
        Ok(())
    }

    fn set_playing(&mut self, play: bool) {
        if self.deck.is_none() {
            return;
        }
        if play && self.state == PlayState::Ended {
            let _ = self.seek(0);
        }
        if play && self.output.is_none() {
            // Device was lost: reopen and continue from the current position.
            let pos = self.position_ms();
            if let Err(e) = self.ensure_output().and_then(|_| self.seek(pos)) {
                self.fail(e);
                return;
            }
        }
        self.state = if play {
            PlayState::Playing
        } else {
            PlayState::Paused
        };
        self.shared.playing.store(play, Ordering::Relaxed);
        self.shared.starving_ok.store(true, Ordering::Relaxed); // re-armed by `pump`
    }

    fn seek(&mut self, ms: u64) -> Result<(), AppError> {
        let Some(fmt) = self.output.as_ref().map(|(o, _)| o.format) else {
            return Ok(());
        };
        let Some(old) = self.deck.take() else {
            return Ok(());
        };
        let ms = old.duration_ms.map_or(ms, |d| ms.min(d));
        // After a chain switch the deck's source is already the next track: reopen this one.
        let mut source = if old.boundaries.is_empty() {
            old.source
        } else {
            Source::open(&old.path)?
        };
        source.seek(ms)?;
        self.flush();
        self.lost_position = None;
        let base = ms * u64::from(fmt.rate) / 1000;
        let mut deck = new_deck(source, old.path, fmt, base)?;
        deck.base_consumed = self.shared.consumed.load(Ordering::Relaxed);
        self.deck = Some(deck);
        self.feeding = None;
        self.next_failed = false;
        self.next_attached = false;
        self.next_source = None;
        if self.state == PlayState::Ended {
            self.state = PlayState::Paused;
        }
        Ok(())
    }

    /// Drops everything queued in the ring (render thread acknowledges within one period).
    fn flush(&mut self) {
        // An empty ring right after a flush is intended, not an underrun; `pump` re-arms.
        self.shared.starving_ok.store(true, Ordering::Relaxed);
        self.flush_seq += 1;
        self.shared
            .flush_req
            .store(self.flush_seq, Ordering::Release);
        let t0 = Instant::now();
        while self.shared.flush_ack.load(Ordering::Acquire) != self.flush_seq
            && t0.elapsed() < Duration::from_millis(200)
            && !self.shared.failed.load(Ordering::Relaxed)
        {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Decodes and pushes while the ring has room: the current deck, then — once it has ended
    /// and a next track is set — the next deck straight after it in the same ring (gapless).
    fn pump(&mut self) -> Result<(), AppError> {
        if !matches!(self.state, PlayState::Playing | PlayState::Paused) {
            return Ok(());
        }
        let playing = self.state == PlayState::Playing;
        let (Some(deck), Some((out, prod))) = (self.deck.as_mut(), self.output.as_mut()) else {
            return Ok(());
        };
        if !self.next_attached
            && !self.next_failed
            && !deck.eof
            && deck.boundaries.is_empty()
            && let Some(path) = self.next_path.clone()
        {
            self.next_attached = true;
            match Source::open(&path) {
                Ok(next)
                    if next.rate == deck.source.rate && next.channels == deck.source.channels =>
                {
                    deck.chained = Some((next, path));
                }
                // Other format: kept open, a second deck built from it takes over at the end.
                Ok(next) => self.next_source = Some((next, path)),
                Err(error) => {
                    self.next_failed = true;
                    (self.events)(EngineEvent::Error {
                        error,
                        path: Some(path),
                    });
                }
            }
        }
        fill(deck, out.format, prod, &self.shared, playing)?;
        if deck.eof
            && self.feeding.is_none()
            && !self.next_failed
            && let Some(path) = self.next_path.clone()
        {
            let opened = match self.next_source.take() {
                Some((src, p)) if p == path => Ok(src),
                _ => Source::open(&path),
            };
            match opened.and_then(|src| new_deck(src, path.clone(), out.format, 0)) {
                Ok(mut next) => {
                    // Audible once everything of the current track has been played.
                    next.base_consumed = deck.base_consumed + deck.pushed;
                    self.feeding = Some(next);
                }
                Err(error) => {
                    self.next_failed = true;
                    (self.events)(EngineEvent::Error {
                        error,
                        path: Some(path),
                    });
                }
            }
        }
        if let Some(next) = self.feeding.as_mut() {
            fill(next, out.format, prod, &self.shared, playing)?;
        }
        Ok(())
    }

    fn check_end(&mut self) {
        let consumed_now = self.shared.consumed.load(Ordering::Relaxed);
        if let Some(deck) = self.deck.as_mut()
            && let Some(b) = deck.boundaries.front()
            && consumed_now.saturating_sub(deck.base_consumed) >= b.at_out
            && let Some(b) = deck.boundaries.pop_front()
        {
            // The chained track's first frame reached the device: rebase the deck on it.
            deck.path = b.path;
            deck.duration_ms = b.duration_ms;
            deck.base_consumed += b.at_out;
            deck.base_frames = 0;
            deck.pushed = deck.pushed.saturating_sub(b.at_out);
            deck.in_frames = deck.in_frames.saturating_sub(b.at_in);
            deck.decoded_in = deck.decoded_in.saturating_sub(b.at_in);
            for rest in deck.boundaries.iter_mut() {
                rest.at_out -= b.at_out;
                rest.at_in -= b.at_in;
            }
            self.next_path = None;
            self.next_attached = false;
            self.next_source = None;
            self.next_failed = false;
            self.emit_snapshot();
            return;
        }
        if let Some(next) = self.feeding.as_ref()
            && consumed_now >= next.base_consumed
        {
            // The first frame of the next track reached the device: it is the current one now.
            self.deck = self.feeding.take();
            self.next_path = None;
            self.next_failed = false;
            self.emit_snapshot();
            return;
        }
        let Some(deck) = self.deck.as_ref() else {
            return;
        };
        if self.state != PlayState::Playing || !deck.eof || self.feeding.is_some() {
            return;
        }
        self.shared.starving_ok.store(true, Ordering::Relaxed);
        let played = self.shared.consumed.load(Ordering::Relaxed) - deck.base_consumed;
        if played >= deck.pushed {
            self.state = PlayState::Ended;
            self.shared.playing.store(false, Ordering::Relaxed);
            self.emit_snapshot();
        }
    }

    /// The system default changed. If our device is still alive the user switched outputs on
    /// purpose: continue there at the same position (≤ 1 s, FR-PLAY-012). If it is gone (pulled
    /// headphones) it is a loss.
    fn on_default_changed(&mut self) {
        let Some(id) = self.output.as_ref().map(|(o, _)| o.device_id.clone()) else {
            return;
        };
        if !crate::devices::is_active(&id) {
            self.on_output_gone();
            return;
        }
        if self.deck.is_none() {
            self.output = None; // reopened lazily on the next Load
            return;
        }
        let pos = self.position_ms();
        self.output = None;
        match self.ensure_output().and_then(|_| self.seek(pos)) {
            Ok(()) => {
                tracing::info!(target: "sirtone::audio", "output moved to the new default device");
                self.emit_snapshot();
            }
            Err(e) => self.fail(e),
        }
    }

    /// Our output device disappeared: pause (default) so the music does not jump to the speakers,
    /// or follow the new default when the user turned that off.
    fn on_output_gone(&mut self) {
        let pos = self.position_ms();
        self.output = None;
        self.default_changed_at = None;
        if self.deck.is_none() {
            return;
        }
        self.lost_position = Some(pos);
        if self.pause_on_device_loss || self.state != PlayState::Playing {
            if self.state == PlayState::Playing {
                self.state = PlayState::Paused;
            }
            self.shared.playing.store(false, Ordering::Relaxed);
            tracing::info!(target: "sirtone::audio", "output device lost — paused");
            self.emit_snapshot();
        } else if let Err(e) = self.ensure_output().and_then(|_| self.seek(pos)) {
            self.fail(e);
        } else {
            self.emit_snapshot();
        }
    }

    fn on_device_lost(&mut self) {
        // Remember where we were; Play reopens the device and seeks here.
        self.lost_position = Some(self.position_ms());
        self.output = None;
        self.shared.failed.store(false, Ordering::Relaxed);
        self.state = PlayState::Paused;
        self.shared.playing.store(false, Ordering::Relaxed);
        (self.events)(EngineEvent::Error {
            error: AppError::new(ErrorCode::DeviceUnavailable).retryable(),
            path: None,
        });
        self.emit_snapshot();
    }

    fn position_ms(&self) -> u64 {
        if let (Some(p), None) = (self.lost_position, self.output.as_ref()) {
            return p;
        }
        let (Some(deck), Some((out, _))) = (self.deck.as_ref(), self.output.as_ref()) else {
            return 0;
        };
        let consumed = self.shared.consumed.load(Ordering::Relaxed);
        let padding = self.shared.padding.load(Ordering::Relaxed);
        let played = (consumed - deck.base_consumed)
            .saturating_sub(padding)
            .min(deck.pushed);
        let ms = (deck.base_frames + played) * 1000 / u64::from(out.format.rate);
        deck.duration_ms.map_or(ms, |d| ms.min(d))
    }

    fn fail(&mut self, error: AppError) {
        let path = self.deck.as_ref().map(|d| d.path.clone());
        self.deck = None;
        self.state = PlayState::Empty;
        self.shared.playing.store(false, Ordering::Relaxed);
        self.shared.starving_ok.store(true, Ordering::Relaxed);
        (self.events)(EngineEvent::Error { error, path });
        self.emit_snapshot();
    }

    fn emit_snapshot(&mut self) {
        self.last_snapshot = Instant::now();
        let snap = Snapshot {
            state: self.state,
            path: self.deck.as_ref().map(|d| d.path.clone()),
            position_ms: if self.state == PlayState::Ended {
                self.deck.as_ref().and_then(|d| d.duration_ms).unwrap_or(0)
            } else {
                self.position_ms()
            },
            duration_ms: self.deck.as_ref().and_then(|d| d.duration_ms),
            volume: self.volume,
            underruns: self.shared.underruns.load(Ordering::Relaxed),
            device: self.output.as_ref().map(|(o, _)| o.device_id.clone()),
        };
        (self.events)(EngineEvent::Snapshot(snap));
    }
}

/// Decodes, maps channels, resamples and pushes `deck` while the ring has room for a chunk.
pub(crate) fn fill(
    deck: &mut Deck,
    fmt: DeviceFormat,
    prod: &mut rtrb::Producer<f32>,
    shared: &Shared,
    playing: bool,
) -> Result<(), AppError> {
    let ch = fmt.channels;
    while !deck.eof {
        let room = prod.slots();
        if room < deck.out_buf.len() {
            break;
        }
        // Fill one resampler chunk of channel-mapped input.
        let need = deck
            .resampler
            .as_ref()
            .map_or(CHUNK, |r| r.input_frames_next());
        while deck.pending.len() < need * ch {
            deck.decoded.clear();
            if !deck.source.next_into(&mut deck.decoded)? {
                if let Some((next, path)) = deck.chained.take() {
                    // Same rate: the next track's first frame follows this one's last frame.
                    let at_in = deck.decoded_in;
                    deck.boundaries.push_back(Boundary {
                        at_out: at_in * u64::from(fmt.rate) / u64::from(deck.source.rate),
                        at_in,
                        path,
                        duration_ms: next.duration_ms,
                    });
                    deck.source = next;
                    continue;
                }
                break;
            }
            deck.decoded_in += (deck.decoded.len() / deck.source.channels.max(1)) as u64;
            map_channels(
                &deck.decoded,
                deck.source.channels,
                deck.source.channel_mask,
                ch,
                &mut deck.pending,
            );
        }
        let have = deck.pending.len() / ch;
        let at_end = have < need;
        // Exactly `need` frames go into the resampler; what the last decoded packet brought
        // beyond that stays in `pending` for the next chunk (dropping it caused audible
        // gaps every ~23 ms on 44.1→48 kHz). Only the very tail is padded with silence.
        let (produced, consumed) = match deck.resampler.as_mut() {
            Some(r) => {
                if at_end {
                    deck.pending.resize(need * ch, 0.0);
                }
                let input = InterleavedSlice::new(&deck.pending[..need * ch], ch, need)
                    .map_err(internal)?;
                let cap = deck.out_buf.len() / ch;
                let mut output =
                    InterleavedSlice::new_mut(&mut deck.out_buf, ch, cap).map_err(internal)?;
                let (_, written) = r
                    .process_into_buffer(&input, &mut output, None)
                    .map_err(internal)?;
                deck.pending.drain(..need * ch);
                (written, have.min(need))
            }
            None => {
                let n = have.min(deck.out_buf.len() / ch);
                deck.out_buf[..n * ch].copy_from_slice(&deck.pending[..n * ch]);
                deck.pending.drain(..n * ch);
                (n, n)
            }
        };
        deck.in_frames += consumed as u64;
        // Exact resampled length of this segment: the resampler's delay is skipped at the
        // start and its flushed zeros are cut at the end.
        let expected = deck.in_frames * u64::from(fmt.rate) / u64::from(deck.source.rate);
        let start = deck.skip_out.min(produced);
        deck.skip_out -= start;
        let allowed = expected.saturating_sub(deck.pushed) as usize;
        let end = produced.min(start + allowed);
        let frames = &deck.out_buf[start * ch..end * ch];
        if let Ok(chunk) = prod.write_chunk_uninit(frames.len()) {
            chunk.fill_from_iter(frames.iter().copied());
        }
        deck.pushed += (end - start) as u64;
        if playing && end > start {
            shared.starving_ok.store(false, Ordering::Relaxed);
        }
        if at_end {
            deck.tail_chunks += 1;
            if deck.resampler.is_none() || deck.pushed >= expected || deck.tail_chunks > 16 {
                deck.eof = true;
            }
        }
    }
    Ok(())
}

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal).with_details(e)
}

pub(crate) fn new_deck(
    source: Source,
    path: PathBuf,
    fmt: DeviceFormat,
    base_frames: u64,
) -> Result<Deck, AppError> {
    let ch = fmt.channels;
    let (resampler, out_max, skip_out) = if source.rate == fmt.rate {
        (None, CHUNK, 0)
    } else {
        let r = Fft::<f32>::new(
            source.rate as usize,
            fmt.rate as usize,
            CHUNK,
            ch,
            FixedSync::Input,
        )
        .map_err(internal)?;
        let max = r.output_frames_max();
        let delay = r.output_delay();
        (Some(r), max, delay)
    };
    let duration_ms = source.duration_ms;
    Ok(Deck {
        duration_ms,
        decoded_in: 0,
        chained: None,
        boundaries: VecDeque::new(),
        source,
        path,
        resampler,
        skip_out,
        pending: Vec::with_capacity(CHUNK * 2 * ch),
        decoded: Vec::new(),
        out_buf: vec![0.0; out_max * ch],
        eof: false,
        in_frames: 0,
        tail_chunks: 0,
        pushed: 0,
        base_frames,
        base_consumed: 0,
    })
}

/// Stereo weights of a speaker position (bit of the WAVE/symphonia channel mask): the default
/// downmix of ffmpeg/libswresample for float output, without normalisation — measured with
/// `ffmpeg -ac 2` on test tones (5.1, 7.1, 7.1(wide), 4.0, 2.1; 2026-10-17). Centre −3 dB to
/// both sides, surrounds −3 dB to their side, rear centre −6 dB to both, LFE dropped.
fn stereo_weights(bit: u32) -> (f32, f32) {
    const H: f32 = std::f32::consts::FRAC_1_SQRT_2;
    match bit {
        0 | 6 => (1.0, 0.0), // FL, FLC
        1 | 7 => (0.0, 1.0), // FR, FRC
        2 => (H, H),         // FC
        4 | 9 => (H, 0.0),   // RL, SL
        5 | 10 => (0.0, H),  // RR, SR
        8 => (0.5, 0.5),     // RC
        _ => (0.0, 0.0),     // LFE, height channels
    }
}

/// The usual layout of a file that does not say (WAVE/Vorbis/FLAC defaults by channel count).
fn default_mask(channels: usize) -> u64 {
    match channels {
        3 => 0x7,   // FL FR FC
        4 => 0x33,  // FL FR RL RR
        5 => 0x37,  // FL FR FC RL RR
        6 => 0x3F,  // 5.1
        7 => 0x70F, // 6.1: FL FR FC LFE RC SL SR
        8 => 0x63F, // 7.1: FL FR FC LFE RL RR SL SR
        _ => 0,
    }
}

/// Source frames → device frames. Mono → both front speakers; stereo → L/R; same count →
/// as is; more source channels → the stereo downmix above (by the channel mask, or the usual
/// layout); extra device channels → silence.
fn map_channels(input: &[f32], in_ch: usize, mask: u64, out_ch: usize, out: &mut Vec<f32>) {
    let in_ch = in_ch.max(1);
    let mask = if mask.count_ones() as usize == in_ch {
        mask
    } else {
        default_mask(in_ch)
    };
    let mut weights = [(0f32, 0f32); 32];
    let mut bits = (0..64u32).filter(|b| mask & (1 << b) != 0);
    for w in weights.iter_mut().take(in_ch.min(32)) {
        *w = bits.next().map_or((0.0, 0.0), stereo_weights);
    }
    for frame in input.chunks_exact(in_ch) {
        if in_ch == out_ch && in_ch > 2 {
            out.extend_from_slice(frame);
            continue;
        }
        let (l, r) = match in_ch {
            1 => (frame[0], frame[0]),
            2 => (frame[0], frame[1]),
            _ => frame
                .iter()
                .zip(weights.iter())
                .fold((0.0, 0.0), |(l, r), (v, (wl, wr))| (l + v * wl, r + v * wr)),
        };
        for c in 0..out_ch {
            out.push(match c {
                0 => l,
                1 => r,
                _ => 0.0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_map_mono_and_surround() {
        let mut out = Vec::new();
        map_channels(&[0.5, -0.5], 1, 0, 2, &mut out);
        assert_eq!(out, [0.5, 0.5, -0.5, -0.5]);
        out.clear();
        map_channels(&[0.1, 0.2], 2, 0, 6, &mut out);
        assert_eq!(out, [0.1, 0.2, 0.0, 0.0, 0.0, 0.0]);
        out.clear();
        map_channels(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6], 6, 0x3F, 6, &mut out);
        assert_eq!(
            out,
            [0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
            "same layout passes through"
        );
    }

    /// The ffmpeg reference (`ffmpeg -ac 2`, float): 5.1 frame [0.1 0.2 0.3 0.4 0.05 0.06]
    /// → [0.34749 0.45456]; all six at 0.4 → 0.9657 per side (no normalisation).
    #[test]
    fn surround_downmix_matches_ffmpeg() {
        let mut out = Vec::new();
        map_channels(&[0.1, 0.2, 0.3, 0.4, 0.05, 0.06], 6, 0x3F, 2, &mut out);
        assert!(
            (out[0] - 0.34749).abs() < 1e-4 && (out[1] - 0.45456).abs() < 1e-4,
            "{out:?}"
        );
        out.clear();
        map_channels(&[0.4; 6], 6, 0, 2, &mut out); // no mask: the usual 5.1
        assert!((out[0] - 0.9657).abs() < 1e-3, "{out:?}");
        out.clear();
        // 7.1(wide): front-left-of-centre goes to the left at full level
        map_channels(
            &[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 0.0],
            8,
            0xFF,
            2,
            &mut out,
        );
        assert_eq!(out, [0.4, 0.0]);
        out.clear();
        // 2.1: LFE dropped
        map_channels(&[0.4, 0.2, 0.1], 3, 0xB, 2, &mut out);
        assert_eq!(out, [0.4, 0.2]);
    }
}
