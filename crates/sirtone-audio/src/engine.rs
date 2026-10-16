//! Engine thread (docs/04 §1): commands in, decode → channel map → resample to the device rate →
//! ring; snapshots and events out. Heavy work lives here, never in the render thread.
//!
//! Prototype scope (0.2): one deck, shared mode, default device. Gapless, automix, EQ, normaliser
//! and the visualiser tap come with T1.5+ and Stage 4.

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
    std::thread::Builder::new()
        .name("sirtone-engine".into())
        .spawn(move || Engine::new(events).run(rx))
        .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))?;
    Ok(EngineHandle { tx })
}

const CHUNK: usize = 1024;
const SNAPSHOT_EVERY: Duration = Duration::from_millis(250);

struct Deck {
    source: Source,
    path: PathBuf,
    resampler: Option<Fft<f32>>,
    /// Device-rate output frames still to drop (resampler delay after reset).
    skip_out: usize,
    /// Channel-mapped, not yet resampled input (interleaved, device channel count).
    pending: Vec<f32>,
    decoded: Vec<f32>,
    out_buf: Vec<f32>,
    eof: bool,
    /// Input frames fed since the segment start (to bound the resampled tail).
    in_frames: u64,
    /// Zero-padded chunks fed after the decoder ended (flushes the resampler delay).
    tail_chunks: u32,
    /// Device-rate frames pushed since the segment start.
    pushed: u64,
    /// Segment start: track position (device frames) and `Shared::consumed` at that moment.
    base_frames: u64,
    base_consumed: u64,
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
        }
    }

    fn run(mut self, rx: mpsc::Receiver<EngineCmd>) {
        loop {
            let busy =
                self.state == PlayState::Playing && self.deck.as_ref().is_some_and(|d| !d.eof);
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
        let ms = old.source.duration_ms.map_or(ms, |d| ms.min(d));
        let mut source = old.source;
        source.seek(ms)?;
        self.flush();
        self.lost_position = None;
        let base = ms * u64::from(fmt.rate) / 1000;
        let mut deck = new_deck(source, old.path, fmt, base)?;
        deck.base_consumed = self.shared.consumed.load(Ordering::Relaxed);
        self.deck = Some(deck);
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

    /// Decodes and pushes while the ring has room for a whole resampled chunk.
    fn pump(&mut self) -> Result<(), AppError> {
        if !matches!(self.state, PlayState::Playing | PlayState::Paused) {
            return Ok(());
        }
        let (Some(deck), Some((out, prod))) = (self.deck.as_mut(), self.output.as_mut()) else {
            return Ok(());
        };
        let ch = out.format.channels;
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
                    break;
                }
                map_channels(&deck.decoded, deck.source.channels, ch, &mut deck.pending);
            }
            let have = deck.pending.len() / ch;
            let at_end = have < need;
            let produced = match deck.resampler.as_mut() {
                Some(r) => {
                    deck.pending.resize(need * ch, 0.0);
                    let input = InterleavedSlice::new(&deck.pending[..need * ch], ch, need)
                        .map_err(internal)?;
                    let cap = deck.out_buf.len() / ch;
                    let mut output =
                        InterleavedSlice::new_mut(&mut deck.out_buf, ch, cap).map_err(internal)?;
                    let (_, written) = r
                        .process_into_buffer(&input, &mut output, None)
                        .map_err(internal)?;
                    deck.pending.clear();
                    written
                }
                None => {
                    let n = have.min(deck.out_buf.len() / ch);
                    deck.out_buf[..n * ch].copy_from_slice(&deck.pending[..n * ch]);
                    deck.pending.drain(..n * ch);
                    n
                }
            };
            deck.in_frames += have as u64;
            // Exact resampled length of this segment: the resampler's delay is skipped at the
            // start and its flushed zeros are cut at the end.
            let expected =
                deck.in_frames * u64::from(out.format.rate) / u64::from(deck.source.rate);
            let start = deck.skip_out.min(produced);
            deck.skip_out -= start;
            let allowed = expected.saturating_sub(deck.pushed) as usize;
            let end = produced.min(start + allowed);
            let frames = &deck.out_buf[start * ch..end * ch];
            if let Ok(chunk) = prod.write_chunk_uninit(frames.len()) {
                chunk.fill_from_iter(frames.iter().copied());
            }
            deck.pushed += (end - start) as u64;
            if self.state == PlayState::Playing && end > start {
                self.shared.starving_ok.store(false, Ordering::Relaxed);
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

    fn check_end(&mut self) {
        let Some(deck) = self.deck.as_ref() else {
            return;
        };
        if self.state != PlayState::Playing || !deck.eof {
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
        deck.source.duration_ms.map_or(ms, |d| ms.min(d))
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
                self.deck
                    .as_ref()
                    .and_then(|d| d.source.duration_ms)
                    .unwrap_or(0)
            } else {
                self.position_ms()
            },
            duration_ms: self.deck.as_ref().and_then(|d| d.source.duration_ms),
            volume: self.volume,
            underruns: self.shared.underruns.load(Ordering::Relaxed),
        };
        (self.events)(EngineEvent::Snapshot(snap));
    }
}

fn internal(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Internal).with_details(e)
}

fn new_deck(
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
    Ok(Deck {
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

/// mono → every front channel, stereo → L/R, more source channels → first two, extra device
/// channels → silence. (Proper down/up-mix matrices: T1.3.)
fn map_channels(input: &[f32], in_ch: usize, out_ch: usize, out: &mut Vec<f32>) {
    let in_ch = in_ch.max(1);
    for frame in input.chunks_exact(in_ch) {
        for c in 0..out_ch {
            let v = if in_ch == 1 {
                if c < 2 { frame[0] } else { 0.0 }
            } else if c < 2 {
                frame[c]
            } else {
                0.0
            };
            out.push(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_map_mono_and_surround() {
        let mut out = Vec::new();
        map_channels(&[0.5, -0.5], 1, 2, &mut out);
        assert_eq!(out, [0.5, 0.5, -0.5, -0.5]);
        out.clear();
        map_channels(&[0.1, 0.2], 2, 6, &mut out);
        assert_eq!(out, [0.1, 0.2, 0.0, 0.0, 0.0, 0.0]);
        out.clear();
        map_channels(&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6], 6, 2, &mut out);
        assert_eq!(out, [0.1, 0.2]);
    }
}
