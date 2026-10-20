//! Engine thread (docs/04 §1): commands in, decode → channel map → resample to the device rate →
//! ring; snapshots and events out. Heavy work lives here, never in the render thread.
//!
//! Prototype scope (0.2): one deck, shared mode, default device. Gapless, automix, EQ, normaliser
//! and the visualiser tap come with T1.5+ and Stage 4.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
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
        /// Normalization gain, dB (T4.11; 0 — none).
        gain_db: f32,
        /// A manual «Следующий» with a short fade (T4.9b, `04` §4.3: foobar2000/Rockbox): the
        /// sounding track fades out over `.0` ms from where it is heard, the new one in over `.1`.
        fade_ms: Option<(u32, u32)>,
    },
    Play,
    Pause,
    Toggle,
    /// Position in milliseconds.
    Seek(u64),
    /// Slider value 0…1; the engine applies the cubic curve (docs/04 §7).
    SetVolume(f32),
    /// Ducking (T4.14): the gain while another application sounds, dB (0 — none).
    SetDuck(f32),
    /// A UI sound (T4.15, id from `sfx::id_of`); dropped if one played < 80 ms ago.
    Sfx(u8),
    /// UI sounds volume 0…1 (0 — off).
    SetSfxVolume(f32),
    /// The sleep timer (T4.21, `14` §14): glide the late multiplier to `target_db` over
    /// `ramp_ms`, linearly in dB (0 dB, 0 ms — untouched).
    SetSleepGain {
        target_db: f32,
        ramp_ms: u32,
    },
    /// «Тёплый звук» (T4.21): the low-pass cut-off to glide to; `None` — off (a true bypass).
    SetWarm(Option<f32>),
    /// The track to continue with when the current one ends, without a gap (T1.5, docs/04 §3:
    /// pre-opened and fed into the same ring), its normalization gain and — for automix or
    /// crossfade (T4.9) — how A hands over to it. `None` clears it.
    SetNext(Option<NextTrack>),
    /// New normalization gains (dB) for these files — the playing one and the next one —
    /// ramped where they already sound (a settings change, shuffle flipping album gain).
    SetGain(Vec<(PathBuf, f32)>),
    /// Equalizer settings (T4.12); glides over 20 ms, off is a true bypass.
    SetEq(crate::eq::EqParams),
    /// The limiter (T4.13): on while anything may raise the level (normalization, EQ). Settings
    /// decide it, not each track — its 1.5 ms delay must not come and go at gapless joints.
    SetLimiter(bool),
    /// FR-PLAY-012: pause when the output device disappears (default on), else follow the new default.
    SetPauseOnDeviceLoss(bool),
    /// From the device watcher: the system default output changed.
    DefaultDeviceChanged,
    /// From the device watcher: an endpoint was unplugged, disabled or removed.
    DeviceLost(String),
    Shutdown,
}

/// The track after the current one.
#[derive(Debug, Clone, PartialEq)]
pub struct NextTrack {
    pub path: PathBuf,
    pub gain_db: f32,
    /// `None` — gapless (L0).
    pub mix: Option<MixSpec>,
}

impl NextTrack {
    pub fn gapless(path: PathBuf, gain_db: f32) -> Self {
        Self {
            path,
            gain_db,
            mix: None,
        }
    }
}

/// A planned handover (`sirtone_playback::automix::Plan` in engine terms, T4.9).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixSpec {
    /// In A: B starts here …
    pub a_out_ms: u64,
    /// … A stops here.
    pub a_stop_ms: u64,
    /// B starts from here (its lead silence skipped).
    pub b_start_ms: u64,
    pub a_fade: bool,
    pub b_fade: bool,
    /// How long the overlap sounds (output ms); with a stretch it differs from `a_stop − a_out`.
    pub out_ms: u64,
    /// L3: A's rate held after the ramp (input per output; 1 — no stretch) and the ramp length.
    pub rate: f64,
    pub ramp_ms: u64,
}

impl MixSpec {
    /// A plain overlap (no stretch): it sounds as long as it is in A.
    pub fn plain(
        a_out_ms: u64,
        a_stop_ms: u64,
        b_start_ms: u64,
        a_fade: bool,
        b_fade: bool,
    ) -> Self {
        Self {
            a_out_ms,
            a_stop_ms,
            b_start_ms,
            a_fade,
            b_fade,
            out_ms: a_stop_ms.saturating_sub(a_out_ms),
            rate: 1.0,
            ramp_ms: 0,
        }
    }
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
    /// What the sound goes through (T4.13, FR-PLAY-002): rates, channels, DSP.
    pub signal: Option<SignalPath>,
    /// The previous track is still sounding under this one (an automix overlap, T4.9): the
    /// journal ends it as «mixed out», not «finished».
    pub mixing: bool,
}

/// The signal path for the «Bit-perfect / Ресемплинг / DSP» indicator (`04` §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalPath {
    pub source_rate: u32,
    pub source_channels: usize,
    pub device_rate: u32,
    pub device_channels: usize,
    /// EQ, normalization gain or the limiter touch the samples.
    pub dsp: bool,
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
    shared: Arc<Shared>,
}

impl EngineHandle {
    pub fn send(&self, cmd: EngineCmd) {
        let _ = self.tx.send(cmd);
    }

    /// A visualiser frame source (T4.1); the tap runs while it lives.
    pub fn viz(&self) -> crate::viz::VizSource {
        crate::viz::VizSource::new(self.shared.clone())
    }
}

/// Starts the engine thread. The output device is opened lazily on the first `Load`.
pub fn start(events: EventSink) -> Result<EngineHandle, AppError> {
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watcher_stop = stop.clone();
    let shared = Shared::new();
    let engine_shared = shared.clone();
    std::thread::Builder::new()
        .name("sirtone-engine".into())
        .spawn(move || {
            Engine::new(events, engine_shared).run(rx);
            stop.store(true, Ordering::Relaxed);
        })
        .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))?;
    let handle = EngineHandle { tx, shared };
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
    pub(crate) pushed: u64,
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
    /// Device-rate frames produced but not yet written to the ring (interleaved).
    pub(crate) ready: Vec<f32>,
    /// Track frame (device rate) where production stops for now (an automix point, T4.9).
    pub(crate) stop_frame: Option<u64>,
    /// Production reached `stop_frame`.
    pub(crate) stopped: bool,
    /// The last written frames before the DSP chain (a ring of `HISTORY` frames): the L3
    /// stretcher is primed with them (T4.9c).
    hist: Vec<f32>,
    hist_pos: usize,
    hist_len: usize,
}

/// Frames of history kept per deck (≈ 85 ms at 48 kHz; the stretcher needs ≥ its input latency).
const HISTORY: usize = 4096;

impl Deck {
    fn remember(&mut self, frames: &[f32], ch: usize) {
        let cap = HISTORY * ch;
        if self.hist.len() != cap {
            self.hist = vec![0.0; cap];
            self.hist_pos = 0;
            self.hist_len = 0;
        }
        for chunk in frames.chunks(ch) {
            let at = self.hist_pos * ch;
            self.hist[at..at + chunk.len()].copy_from_slice(chunk);
            self.hist_pos = (self.hist_pos + 1) % HISTORY;
            self.hist_len = (self.hist_len + 1).min(HISTORY);
        }
    }

    /// The history, oldest first.
    fn history(&self, ch: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.hist_len * ch);
        let start = (self.hist_pos + HISTORY - self.hist_len) % HISTORY;
        for i in 0..self.hist_len {
            let at = ((start + i) % HISTORY) * ch;
            out.extend_from_slice(&self.hist[at..at + ch]);
        }
        out
    }
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
    /// Normalization gains (linear) by file, latest first: every source opened gets its own.
    gains: Vec<(PathBuf, f32)>,
    /// The DSP chain (EQ T4.12, limiter T4.13) for the current device format, and its settings.
    eq: Option<((u32, usize), Chain)>,
    eq_params: crate::eq::EqParams,
    limiter_on: bool,
    /// «Тёплый звук» cut-off while the sleep timer fades (T4.21).
    warm: Option<f32>,
    /// How the current track hands over to the next one (T4.9); `None` — gapless.
    next_mix: Option<MixSpec>,
    /// When the last UI sound was sent (T4.15: ≥ 80 ms apart).
    sfx_at: Option<Instant>,
    /// The overlap in progress: the outgoing deck mixed under the new current one.
    mixing: Option<Mixing>,
}

/// An automix / crossfade overlap (T4.9): A (outgoing) and the current deck B are summed with
/// their curves for `overlap` device frames, then A is dropped.
pub(crate) struct Mixing {
    pub(crate) a: Deck,
    pub(crate) overlap: u64,
    mixed: u64,
    a_fade: bool,
    b_fade: bool,
    /// B's fade-in length (frames); 0 — the whole overlap.
    pub(crate) b_in: u64,
    buf: Vec<f32>,
    /// L3: A goes through the stretcher (T4.9c).
    pub(crate) st: Option<StretchRun>,
}

/// A stretched through the overlap (T4.9c, D-100): primed with A's history, warmed up at rate 1
/// until its output reaches the entry point, then ramped 1 → `rate` over `ramp` output frames
/// and held.
pub(crate) struct StretchRun {
    st: crate::stretch::Stretch,
    rate: f64,
    ramp: f64,
    /// Output frames still to drop (the warm-up).
    warm: usize,
    /// Overlap output frames produced (after the warm-up).
    produced: u64,
    acc: f64,
    out: Vec<f32>,
    /// Stretched A frames not yet mixed.
    ready: Vec<f32>,
}

const STRETCH_BLOCK: usize = 256;

impl StretchRun {
    /// `None` — the history is too short to prime (the overlap then plays without a stretch).
    pub(crate) fn start(a: &Deck, fmt: DeviceFormat, rate: f64, ramp_frames: u64) -> Option<Self> {
        let ch = fmt.channels.max(1);
        let mut st = crate::stretch::Stretch::new(ch, fmt.rate)?;
        let (li, lo) = (st.input_latency(), st.output_latency());
        let hist = a.history(ch);
        let h = hist.len() / ch;
        if h < li {
            return None;
        }
        let mut pre = hist[..li * ch].to_vec();
        st.seek(&mut pre, 1.0);
        let mut rest = hist[li * ch..].to_vec();
        let mut dropped = vec![0f32; rest.len()];
        st.process(&mut rest, &mut dropped);
        Some(Self {
            st,
            rate,
            ramp: ramp_frames.max(1) as f64,
            warm: li + lo,
            produced: 0,
            acc: 0.0,
            out: vec![0f32; STRETCH_BLOCK * ch],
            ready: Vec::new(),
        })
    }

    /// Stretches A until `want` frames are ready (A past its end is silence).
    fn fill(&mut self, a: &mut Deck, fmt: DeviceFormat, want: usize) -> Result<(), AppError> {
        let ch = fmt.channels.max(1);
        while self.ready.len() / ch < want {
            let s = if self.warm > 0 {
                1.0
            } else {
                1.0 + (self.rate - 1.0) * (self.produced as f64 / self.ramp).min(1.0)
            };
            let n_in = if self.warm > 0 {
                STRETCH_BLOCK
            } else {
                self.acc += STRETCH_BLOCK as f64 * s;
                let k = self.acc.floor();
                self.acc -= k;
                k as usize
            };
            while a.ready.len() / ch < n_in && !a.eof {
                produce(a, fmt)?;
            }
            let take = (a.ready.len() / ch).min(n_in);
            let mut input: Vec<f32> = a.ready.drain(..take * ch).collect();
            input.resize(n_in * ch, 0.0);
            self.st.process(&mut input, &mut self.out);
            let skip = self.warm.min(STRETCH_BLOCK);
            self.warm -= skip;
            self.ready.extend_from_slice(&self.out[skip * ch..]);
            self.produced += (STRETCH_BLOCK - skip) as u64;
        }
        Ok(())
    }
}

impl Mixing {
    pub(crate) fn new(a: Deck, overlap: u64, a_fade: bool, b_fade: bool) -> Self {
        Self {
            a,
            overlap,
            mixed: 0,
            a_fade,
            b_fade,
            b_in: 0,
            buf: Vec::new(),
            st: None,
        }
    }
}

/// Equal-power curves (`04` §4.4): out `cos(πt/2)`, in `sin(πt/2)`; `Keep` = 1.
fn curve(t: f64, fade: bool, out: bool) -> f32 {
    if !fade {
        return 1.0;
    }
    let a = std::f64::consts::FRAC_PI_2 * t.clamp(0.0, 1.0);
    (if out { a.cos() } else { a.sin() }) as f32
}

/// What runs on the engine thread after the resampler, before the ring.
pub(crate) struct Chain {
    eq: crate::eq::Eq,
    warm: crate::warm::Warm,
    limiter: crate::limiter::Limiter,
    limiter_on: bool,
}

impl Chain {
    pub(crate) fn process(&mut self, buf: &mut [f32]) {
        self.eq.process(buf);
        self.warm.process(buf);
        if self.limiter_on {
            self.limiter.process(buf);
        }
    }
}

/// A live gain change is ramped over this long (no click).
const GAIN_RAMP_MS: u32 = 30;

impl Engine {
    fn new(events: EventSink, shared: Arc<Shared>) -> Self {
        Self {
            events,
            shared,
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
            gains: Vec::new(),
            eq: None,
            eq_params: crate::eq::EqParams::default(),
            limiter_on: false,
            warm: None,
            next_mix: None,
            mixing: None,
            sfx_at: None,
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
            EngineCmd::Load {
                path,
                autoplay,
                gain_db,
                fade_ms,
            } => {
                self.remember_gain(&path, gain_db);
                let faded = fade_ms.is_some_and(|f| self.load_faded(&path, f));
                if !faded && let Err(e) = self.load(path.clone(), autoplay) {
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
            EngineCmd::Sfx(id) => {
                // `06` §8: at least 80 ms between UI sounds.
                if self
                    .sfx_at
                    .is_none_or(|t| t.elapsed() >= Duration::from_millis(80))
                    && f32::from_bits(self.shared.sfx_bits.load(Ordering::Relaxed)) > 0.0
                    && self.ensure_output().is_ok()
                    && let Some((out, _)) = self.output.as_mut()
                {
                    let _ = out.sfx.push(id);
                    self.sfx_at = Some(Instant::now());
                }
                return;
            }
            EngineCmd::SetSfxVolume(v) => {
                let g = v.clamp(0.0, 1.0);
                self.shared.sfx_bits.store(g.to_bits(), Ordering::Relaxed);
                return;
            }
            EngineCmd::SetSleepGain { target_db, ramp_ms } => {
                self.shared.sleep_bits.store(
                    crate::output::pack_sleep(target_db, ramp_ms),
                    Ordering::Relaxed,
                );
                return;
            }
            EngineCmd::SetWarm(cutoff) => {
                self.warm = cutoff;
                if let Some((_, c)) = self.eq.as_mut() {
                    c.warm.set(cutoff);
                }
            }
            EngineCmd::SetDuck(db) => {
                let amp = sirtone_core::loudness::amplitude(db.min(0.0));
                self.shared
                    .duck_bits
                    .store(amp.to_bits(), Ordering::Relaxed);
                return;
            }
            EngineCmd::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0);
                self.shared
                    .gain_bits
                    .store(self.volume.powi(3).to_bits(), Ordering::Relaxed);
            }
            EngineCmd::SetNext(next) => {
                let mix = next.as_ref().and_then(|n| n.mix);
                let path = next.map(|n| {
                    self.remember_gain(&n.path, n.gain_db);
                    n.path
                });
                if self.next_mix != mix {
                    // A new plan replaces the old one until the overlap begins.
                    self.next_mix = mix;
                    if let Some(d) = self.deck.as_mut() {
                        d.stop_frame = None;
                        d.stopped = false;
                        d.chained = None;
                    }
                    self.next_attached = false;
                }
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
            EngineCmd::SetGain(list) => {
                for (path, db) in list {
                    self.remember_gain(&path, db);
                    let amp = sirtone_core::loudness::amplitude(db);
                    let mut sources: Vec<&mut Source> = Vec::new();
                    for d in [self.deck.as_mut(), self.feeding.as_mut()]
                        .into_iter()
                        .flatten()
                    {
                        sources.push(&mut d.source);
                        if let Some((c, _)) = d.chained.as_mut() {
                            sources.push(c);
                        }
                    }
                    if let Some((n, _)) = self.next_source.as_mut() {
                        sources.push(n);
                    }
                    for s in sources.into_iter().filter(|s| s.path == path) {
                        s.set_gain(amp, GAIN_RAMP_MS);
                    }
                }
                return;
            }
            EngineCmd::SetEq(p) => {
                if let Some((_, c)) = self.eq.as_mut() {
                    c.eq.set(&p);
                }
                self.eq_params = p;
            }
            EngineCmd::SetLimiter(on) => {
                if let Some((_, c)) = self.eq.as_mut()
                    && c.limiter_on != on
                {
                    c.limiter.reset();
                    c.limiter_on = on;
                }
                self.limiter_on = on;
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

    /// Keeps the gain of `path` for when its source is opened (or reopened after a seek).
    fn remember_gain(&mut self, path: &Path, db: f32) {
        self.gains.retain(|(p, _)| p != path);
        self.gains.insert(
            0,
            (path.to_path_buf(), sirtone_core::loudness::amplitude(db)),
        );
        self.gains.truncate(8);
    }

    /// «Следующий» with a fade (T4.9b): the ring is dropped (the new track starts within the
    /// flush, not after 200 ms of queued sound), the current track is decoded again from the
    /// point being heard and fades out under the new one. `false` — nothing playing to fade, the
    /// caller loads plainly.
    fn load_faded(&mut self, path: &Path, (out_ms, in_ms): (u32, u32)) -> bool {
        if self.state != PlayState::Playing || self.mixing.is_some() {
            return false;
        }
        let Some(fmt) = self.output.as_ref().map(|(o, _)| o.format) else {
            return false;
        };
        let pos = self.position_ms();
        let Some(old) = self.deck.take() else {
            return false;
        };
        let rate = u64::from(fmt.rate);
        let opened = (|| -> Result<(Deck, Deck), AppError> {
            let mut src = if old.boundaries.is_empty() {
                old.source
            } else {
                open_with_gain(&self.gains, &old.path)?
            };
            src.seek(pos)?;
            let a = new_deck(src, old.path.clone(), fmt, pos * rate / 1000)?;
            let b = new_deck(
                open_with_gain(&self.gains, path)?,
                path.to_path_buf(),
                fmt,
                0,
            )?;
            Ok((a, b))
        })();
        let Ok((mut a, mut b)) = opened else {
            return false;
        };
        self.flush();
        let now = self.shared.consumed.load(Ordering::Relaxed);
        let overlap = u64::from(out_ms) * rate / 1000;
        a.base_consumed = now;
        a.stop_frame = Some(a.base_frames + overlap);
        b.base_consumed = now;
        let mut m = Mixing::new(a, overlap, true, true);
        m.b_in = u64::from(in_ms) * rate / 1000;
        self.mixing = Some(m);
        self.deck = Some(b);
        self.feeding = None;
        self.next_mix = None;
        self.next_path = None;
        self.next_attached = false;
        self.next_source = None;
        self.next_failed = false;
        self.lost_position = None;
        true
    }

    fn load(&mut self, path: PathBuf, autoplay: bool) -> Result<(), AppError> {
        let source = open_with_gain(&self.gains, &path)?;
        let fmt = self.ensure_output()?;
        self.flush();
        self.mixing = None;
        self.next_mix = None;
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
            open_with_gain(&self.gains, &old.path)?
        };
        source.seek(ms)?;
        self.flush();
        // A seek ends an overlap: the outgoing track is gone, the plan for the next is redone.
        self.mixing = None;
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
        // Nothing of the old sound may come out of the limiter's delay line.
        if let Some((_, c)) = self.eq.as_mut() {
            c.limiter.reset();
        }
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
        // The equalizer follows the device format (a new device → new filters, same settings).
        let key = (out.format.rate, out.format.channels);
        if self.eq.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut eq = crate::eq::Eq::new(key.0, key.1);
            eq.set(&self.eq_params);
            let mut warm = crate::warm::Warm::new(key.0, key.1);
            warm.set(self.warm);
            let chain = Chain {
                eq,
                warm,
                limiter: crate::limiter::Limiter::new(key.0, key.1),
                limiter_on: self.limiter_on,
            };
            self.eq = Some((key, chain));
        }
        // A planned handover (T4.9): A plays up to the point where B comes in.
        if let Some(m) = self.next_mix
            && self.mixing.is_none()
            && deck.stop_frame.is_none()
            && deck.boundaries.is_empty()
        {
            deck.stop_frame = Some(m.a_out_ms * u64::from(out.format.rate) / 1000);
        }
        if self.next_mix.is_none()
            && !self.next_attached
            && !self.next_failed
            && !deck.eof
            && deck.boundaries.is_empty()
            && let Some(path) = self.next_path.clone()
        {
            self.next_attached = true;
            match open_with_gain(&self.gains, &path) {
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
        if let Some(m) = self.mixing.as_mut() {
            let done = mix_fill(
                m,
                deck,
                out.format,
                prod,
                &self.shared,
                playing,
                self.eq.as_mut().map(|(_, e)| e),
            )?;
            if done {
                self.mixing = None;
                self.emit_snapshot();
            }
            return Ok(());
        }
        let eq = self.eq.as_mut().map(|(_, e)| e);
        fill(deck, out.format, prod, &self.shared, playing, eq)?;
        // The automix point reached and written: B comes in.
        if deck.stopped
            && let (Some(m), Some(path)) = (self.next_mix, self.next_path.clone())
        {
            let fmt = out.format;
            let rate = u64::from(fmt.rate);
            let opened = open_with_gain(&self.gains, &path).and_then(|mut src| {
                if m.b_start_ms > 0 {
                    src.seek(m.b_start_ms)?;
                }
                new_deck(src, path.clone(), fmt, m.b_start_ms * rate / 1000)
            });
            match opened {
                Ok(mut b) => {
                    let ch = fmt.channels.max(1);
                    let unwritten = (deck.ready.len() / ch) as u64;
                    let at = deck.base_frames + deck.pushed - unwritten;
                    let stop = (m.a_stop_ms * rate / 1000).max(at);
                    let out = m.out_ms * rate / 1000;
                    b.base_consumed = deck.base_consumed + deck.pushed - unwritten;
                    let Some(mut a) = self.deck.take() else {
                        return Ok(());
                    };
                    let st = (m.rate != 1.0)
                        .then(|| StretchRun::start(&a, fmt, m.rate, m.ramp_ms * rate / 1000))
                        .flatten();
                    // A stretched reads past the stop (the stretcher's lookahead): no stop then.
                    a.stop_frame = if st.is_some() { None } else { Some(stop) };
                    a.stopped = false;
                    let overlap = if st.is_some() { out } else { stop - at };
                    let mut mix = Mixing::new(a, overlap, m.a_fade, m.b_fade);
                    mix.st = st;
                    self.mixing = Some(mix);
                    self.deck = Some(b);
                    self.next_mix = None;
                    self.next_path = None;
                    self.next_attached = false;
                    self.next_source = None;
                    self.next_failed = false;
                    tracing::info!(target: "sirtone::audio", out_ms = m.out_ms, stretch = m.rate, "mix");
                    self.emit_snapshot();
                }
                Err(error) => {
                    // B cannot be opened: A just plays to its end.
                    self.next_mix = None;
                    self.next_failed = true;
                    deck.stop_frame = None;
                    deck.stopped = false;
                    (self.events)(EngineEvent::Error {
                        error,
                        path: Some(path),
                    });
                }
            }
            return Ok(());
        }
        if deck.eof
            && self.feeding.is_none()
            && !self.next_failed
            && let Some(path) = self.next_path.clone()
        {
            let opened = match self.next_source.take() {
                Some((src, p)) if p == path => Ok(src),
                _ => open_with_gain(&self.gains, &path),
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
            fill(
                next,
                out.format,
                prod,
                &self.shared,
                playing,
                self.eq.as_mut().map(|(_, e)| e),
            )?;
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
            signal: match (self.deck.as_ref(), self.output.as_ref()) {
                (Some(d), Some((o, _))) => Some(SignalPath {
                    source_rate: d.source.rate,
                    source_channels: d.source.channels,
                    device_rate: o.format.rate,
                    device_channels: o.format.channels,
                    dsp: self.limiter_on
                        || self.eq_params.enabled
                        || self.warm.is_some()
                        || d.source.gain_db() != 0.0,
                }),
                _ => None,
            },
            mixing: self.mixing.is_some(),
        };
        (self.events)(EngineEvent::Snapshot(snap));
    }
}

/// Decodes, maps channels, resamples and pushes `deck` while the ring has room (through the DSP
/// chain), up to its `stop_frame`.
pub(crate) fn fill(
    deck: &mut Deck,
    fmt: DeviceFormat,
    prod: &mut rtrb::Producer<f32>,
    shared: &Shared,
    playing: bool,
    mut eq: Option<&mut Chain>,
) -> Result<(), AppError> {
    let ch = fmt.channels.max(1);
    loop {
        let can = writable(deck, ch);
        if can == 0 {
            if deck.stop_frame.is_some() && (deck.ready.len() >= ch || deck.eof) {
                // At the automix point (or the track ended before it).
                deck.stopped = true;
                break;
            }
            if deck.eof || prod.slots() < deck.out_buf.len() {
                break;
            }
            produce(deck, fmt)?;
            continue;
        }
        let n = (prod.slots() / ch).min(can);
        if n == 0 {
            break;
        }
        let written = deck.ready[..n * ch].to_vec();
        deck.remember(&written, ch);
        // The DSP chain (EQ, limiter) at the device rate, before the ring.
        if let Some(e) = eq.as_deref_mut() {
            e.process(&mut deck.ready[..n * ch]);
        }
        if let Ok(chunk) = prod.write_chunk_uninit(n * ch) {
            chunk.fill_from_iter(deck.ready[..n * ch].iter().copied());
        }
        deck.ready.drain(..n * ch);
        if playing {
            shared.starving_ok.store(false, Ordering::Relaxed);
        }
    }
    Ok(())
}

/// The overlap (T4.9): A's and B's frames summed with their curves, through the chain, into the
/// ring. `true` once the overlap is complete (A is then dropped).
pub(crate) fn mix_fill(
    m: &mut Mixing,
    b: &mut Deck,
    fmt: DeviceFormat,
    prod: &mut rtrb::Producer<f32>,
    shared: &Shared,
    playing: bool,
    mut eq: Option<&mut Chain>,
) -> Result<bool, AppError> {
    let ch = fmt.channels.max(1);
    loop {
        if m.mixed >= m.overlap {
            return Ok(true);
        }
        let stretched = m.st.is_some();
        let a_can = if stretched {
            usize::MAX
        } else {
            writable(&m.a, ch)
        };
        let a_done = !stretched && a_can == 0 && !wants_more(&m.a, ch);
        if a_can == 0 && !a_done {
            produce(&mut m.a, fmt)?;
            continue;
        }
        if b.ready.is_empty() {
            if b.eof {
                return Ok(true);
            }
            produce(b, fmt)?;
            continue;
        }
        let room = prod.slots() / ch;
        let a_have = if a_done { usize::MAX } else { a_can };
        let mut n = room
            .min(a_have)
            .min(b.ready.len() / ch)
            .min((m.overlap - m.mixed) as usize);
        if n == 0 {
            return Ok(false);
        }
        if let Some(sr) = m.st.as_mut() {
            sr.fill(&mut m.a, fmt, n)?;
            n = n.min(sr.ready.len() / ch);
        }
        m.buf.clear();
        m.buf.extend_from_slice(&b.ready[..n * ch]);
        let total = m.overlap.max(1) as f64;
        let b_total = if m.b_in == 0 { total } else { m.b_in as f64 };
        for f in 0..n {
            let at = (m.mixed + f as u64) as f64;
            let (ga, gb) = (
                curve(at / total, m.a_fade, true),
                curve(at / b_total, m.b_fade, false),
            );
            for c in 0..ch {
                let i = f * ch + c;
                let a = match m.st.as_ref() {
                    Some(sr) => sr.ready[i],
                    None if a_done => 0.0,
                    None => m.a.ready[i],
                };
                m.buf[i] = a * ga + m.buf[i] * gb;
            }
        }
        if let Some(e) = eq.as_deref_mut() {
            e.process(&mut m.buf);
        }
        if let Ok(chunk) = prod.write_chunk_uninit(n * ch) {
            chunk.fill_from_iter(m.buf.iter().copied());
        }
        if let Some(sr) = m.st.as_mut() {
            sr.ready.drain(..n * ch);
        } else if !a_done {
            m.a.ready.drain(..n * ch);
        }
        b.ready.drain(..n * ch);
        m.mixed += n as u64;
        if playing {
            shared.starving_ok.store(false, Ordering::Relaxed);
        }
    }
}

/// Frames of `deck.ready` that may still go out before its `stop_frame` (all without one).
fn writable(deck: &Deck, ch: usize) -> usize {
    let ready = deck.ready.len() / ch;
    match deck.stop_frame {
        Some(stop) => {
            let written = deck.base_frames + deck.pushed - ready as u64;
            ready.min(stop.saturating_sub(written) as usize)
        }
        None => ready,
    }
}

/// Whether production should go on: no stop, or the ready frames do not reach it yet.
fn wants_more(deck: &Deck, ch: usize) -> bool {
    !deck.eof && writable(deck, ch) == deck.ready.len() / ch
}

/// One resampler chunk of device-rate frames into `deck.ready`.
fn produce(deck: &mut Deck, fmt: DeviceFormat) -> Result<(), AppError> {
    let ch = fmt.channels;
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
        let from = deck.pending.len();
        map_channels(
            &deck.decoded,
            deck.source.channels,
            deck.source.channel_mask,
            ch,
            &mut deck.pending,
        );
        // Normalization (T4.11) before the resampler: a chained track switches its gain
        // exactly at its first frame.
        deck.source.apply_gain(&mut deck.pending[from..], ch);
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
            let input =
                InterleavedSlice::new(&deck.pending[..need * ch], ch, need).map_err(internal)?;
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
    deck.ready
        .extend_from_slice(&deck.out_buf[start * ch..end * ch]);
    deck.pushed += (end - start) as u64;
    if at_end {
        deck.tail_chunks += 1;
        if deck.resampler.is_none() || deck.pushed >= expected || deck.tail_chunks > 16 {
            deck.eof = true;
        }
    }
    Ok(())
}

/// Opens a track with its normalization gain (T4.11), none if unknown.
fn open_with_gain(gains: &[(PathBuf, f32)], path: &Path) -> Result<Source, AppError> {
    let mut s = Source::open(path)?;
    let amp = gains
        .iter()
        .find(|(p, _)| p == path)
        .map_or(1.0, |(_, a)| *a);
    s.set_gain(amp, 0);
    Ok(s)
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
        ready: Vec::with_capacity(out_max * ch),
        stop_frame: None,
        stopped: false,
        hist: Vec::new(),
        hist_pos: 0,
        hist_len: 0,
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
