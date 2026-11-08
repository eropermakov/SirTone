//! The phone player's core (M3, FR-MOB-011): decoding runs on a core thread into a lock-free ring
//! (`rtrb`); the platform's real-time render callback (`AVAudioSourceNode` on iPhone, AAudio on
//! Android) pulls samples through [`sirtone_render`] — a plain C function: no allocation, no
//! lock, no logging, no I/O, as the PC audio thread (project rule). Decoders are the PC's
//! (`sirtone-audio`: symphonia + libopus, gapless trims); the audio is never re-encoded.
//!
//! Format: the core outputs the track's native rate and channel count ([`Player::open`] returns
//! them); the app sets the source node to that format and the system converts to the hardware.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};
use sirtone_audio::decode::Source;
use sirtone_audio::eq::{Eq, EqParams};
use sirtone_audio::vizframe::{FRAME, FrameBuilder};

use crate::CoreError;

/// Ring length: 2 s at 48 kHz stereo — enough to ride out a busy moment on the decode thread.
const RING_SAMPLES: usize = 48_000 * 2 * 2;

/// State shared with the render callback (atomics only).
struct Shared {
    paused: AtomicBool,
    /// Volume as `f32` bits (0…1), applied in the callback (a «late» multiplier).
    volume: AtomicU32,
    /// Frames handed to the platform since the last open/seek.
    frames_out: AtomicU64,
    /// Decoding reached the end and the ring is drained.
    ended: AtomicBool,
    decode_done: AtomicBool,
    stop: AtomicBool,
    /// Open visualisers ([`VizFeed`]); the callback copies samples aside only while > 0.
    viz_on: AtomicU32,
    /// The side ring of the current render handle, for the visualiser thread (never locked by
    /// the callback: a new one is put here when a track opens or seeks).
    viz_tap: Mutex<Option<VizTap>>,
    /// The equalizer (D-148, the PC's `sirtone_audio::eq`): set by the app, picked up by the
    /// decode thread at its next chunk (`eq_gen` changes).
    eq: Mutex<EqParams>,
    eq_gen: AtomicU32,
    /// Normalization gain for the current track, dB as `f32` bits; `gain_gen` changes on set.
    gain_db: AtomicU32,
    gain_gen: AtomicU32,
    /// The transition fade (`Player::fade_to`): target 0…1 as bits, its length, a counter the
    /// callback watches; `fade_now` — where the callback is (a new handle starts there).
    fade_target: AtomicU32,
    fade_ms: AtomicU32,
    fade_gen: AtomicU32,
    fade_now: AtomicU32,
    /// How far ahead the decode thread fills, ms (`Player::set_fill_ms`).
    fill_ms: AtomicU32,
}

/// The fill on screen: a setting is heard within this. Off screen or locked the app asks for
/// more (`set_fill_ms`), as П2 held with 2 s.
const FILL_MS: usize = 400;

/// The visualiser's end of the side ring, with the track's format.
struct VizTap {
    consumer: Consumer<f32>,
    rate: u32,
    channels: usize,
}

/// Side ring: half a second at 48 kHz stereo.
const VIZ_RING: usize = 48_000;

/// What the render callback owns. Handed to the app once as an opaque pointer.
pub struct RenderHandle {
    consumer: Consumer<f32>,
    shared: Arc<Shared>,
    rate: u32,
    /// The fade the callback applies now, its step per frame, its goal and the request seen.
    fade: f32,
    fade_step: f32,
    fade_to: f32,
    fade_seen: u32,
    /// Samples handed to the device, before the volume, for the visualiser (D-148).
    viz: Producer<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct TrackFormat {
    pub sample_rate: u32,
    pub channels: u32,
    pub duration_ms: Option<u64>,
    pub codec: String,
}

#[derive(uniffi::Object)]
pub struct Player {
    shared: Arc<Shared>,
    inner: Mutex<Inner>,
}

struct Inner {
    format: Option<TrackFormat>,
    decoder: Option<JoinHandle<()>>,
    /// The render side of the current track until the app takes it.
    handle: Option<Box<RenderHandle>>,
    base_ms: u64,
    path: Option<PathBuf>,
}

fn err(code: &str) -> CoreError {
    CoreError::Failed { code: code.into() }
}

fn decode_loop(mut src: Source, mut producer: Producer<f32>, shared: Arc<Shared>) {
    let mut buf: Vec<f32> = Vec::with_capacity(16_384);
    let ch = src.channels.max(1);
    let rate = src.rate as usize;
    let fill_of = |ms: u32| (rate * ch * ms as usize / 1000).min(RING_SAMPLES * 3 / 4);
    let mut eq = Eq::new(src.rate, ch);
    let (mut eq_seen, mut eq_on) = (u32::MAX, false);
    let mut gain_seen = u32::MAX;
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        let g = shared.gain_gen.load(Ordering::Acquire);
        if g != gain_seen {
            let db = f32::from_bits(shared.gain_db.load(Ordering::Acquire));
            // The first value at once (nothing heard yet), later ones ramped — no click.
            let ramp = if gain_seen == u32::MAX { 0 } else { 300 };
            src.set_gain(sirtone_core::loudness::amplitude(db), ramp);
            gain_seen = g;
        }
        let e = shared.eq_gen.load(Ordering::Acquire);
        if e != eq_seen {
            if let Ok(p) = shared.eq.lock() {
                eq.set(&p);
                eq_on = p.enabled;
            }
            eq_seen = e;
        }
        // Not further ahead than the fill: settings are heard soon, the ring rides out stalls.
        if RING_SAMPLES - producer.slots() >= fill_of(shared.fill_ms.load(Ordering::Relaxed)) {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        if buf.is_empty() {
            match src.next_into(&mut buf) {
                Ok(true) => {
                    if eq_on {
                        eq.process(&mut buf);
                    }
                }
                Ok(false) | Err(_) => {
                    // End or an unreadable file: the callback drains what is left, then «ended».
                    shared.decode_done.store(true, Ordering::Release);
                    return;
                }
            }
        }
        let n = producer.slots().min(buf.len());
        if n == 0 {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        if let Ok(mut chunk) = producer.write_chunk_uninit(n) {
            let (a, b) = chunk.as_mut_slices();
            for (d, s) in a.iter_mut().chain(b.iter_mut()).zip(&buf[..n]) {
                d.write(*s);
            }
            // SAFETY: all `n` slots were written just above.
            unsafe { chunk.commit_all() };
            buf.drain(..n);
        }
    }
}

impl Player {
    fn stop_decoder(inner: &mut Inner, shared: &Shared) {
        shared.stop.store(true, Ordering::Release);
        if let Some(t) = inner.decoder.take() {
            let _ = t.join();
        }
        shared.stop.store(false, Ordering::Release);
    }

    fn start(
        &self,
        inner: &mut Inner,
        path: PathBuf,
        from_ms: u64,
    ) -> Result<TrackFormat, CoreError> {
        Self::stop_decoder(inner, &self.shared);
        let mut src = Source::open(&path).map_err(|_| err("player.open"))?;
        if from_ms > 0 {
            src.seek(from_ms).map_err(|_| err("player.seek"))?;
        }
        let format = TrackFormat {
            sample_rate: src.rate,
            channels: src.channels as u32,
            duration_ms: src.duration_ms,
            codec: src.codec.clone(),
        };
        let (producer, consumer) = RingBuffer::<f32>::new(RING_SAMPLES);
        self.shared.frames_out.store(0, Ordering::Release);
        self.shared.ended.store(false, Ordering::Release);
        self.shared.decode_done.store(false, Ordering::Release);
        let shared = self.shared.clone();
        inner.decoder = Some(
            std::thread::Builder::new()
                .name("sirtone-decode".into())
                .spawn(move || decode_loop(src, producer, shared))
                .map_err(|_| err("player.thread"))?,
        );
        let (viz, viz_consumer) = RingBuffer::<f32>::new(VIZ_RING);
        *self
            .shared
            .viz_tap
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(VizTap {
            consumer: viz_consumer,
            rate: format.sample_rate,
            channels: format.channels as usize,
        });
        let fade = f32::from_bits(self.shared.fade_now.load(Ordering::Acquire));
        inner.handle = Some(Box::new(RenderHandle {
            consumer,
            shared: self.shared.clone(),
            rate: format.sample_rate.max(1),
            fade,
            fade_step: 0.0,
            fade_to: fade,
            fade_seen: self.shared.fade_gen.load(Ordering::Acquire),
            viz,
        }));
        inner.base_ms = from_ms;
        inner.path = Some(path);
        inner.format = Some(format.clone());
        Ok(format)
    }
}

#[uniffi::export]
impl Player {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            shared: Arc::new(Shared {
                paused: AtomicBool::new(true),
                volume: AtomicU32::new(1.0f32.to_bits()),
                frames_out: AtomicU64::new(0),
                ended: AtomicBool::new(false),
                decode_done: AtomicBool::new(false),
                stop: AtomicBool::new(false),
                viz_on: AtomicU32::new(0),
                viz_tap: Mutex::new(None),
                eq: Mutex::new(EqParams::default()),
                eq_gen: AtomicU32::new(0),
                gain_db: AtomicU32::new(0f32.to_bits()),
                gain_gen: AtomicU32::new(0),
                fade_target: AtomicU32::new(1f32.to_bits()),
                fade_ms: AtomicU32::new(0),
                fade_gen: AtomicU32::new(0),
                fade_now: AtomicU32::new(1f32.to_bits()),
                fill_ms: AtomicU32::new(FILL_MS as u32),
            }),
            inner: Mutex::new(Inner {
                format: None,
                decoder: None,
                handle: None,
                base_ms: 0,
                path: None,
            }),
        })
    }

    /// Opens a file (paused). Then the app takes [`Player::take_render_handle`] for its node.
    pub fn open(&self, path: String) -> Result<TrackFormat, CoreError> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.shared.paused.store(true, Ordering::Release);
        self.start(&mut inner, PathBuf::from(path), 0)
    }

    /// The render side for the platform callback, as an address for [`sirtone_render`]; once per
    /// open/seek. The app must call [`sirtone_render_release`] when its node is done with it.
    pub fn take_render_handle(&self) -> u64 {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .handle
            .take()
            .map_or(0, |h| Box::into_raw(h) as usize as u64)
    }

    pub fn play(&self) {
        self.shared.paused.store(false, Ordering::Release);
    }

    pub fn pause(&self) {
        self.shared.paused.store(true, Ordering::Release);
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Release);
    }

    /// Accurate seek: restarts decoding at `ms` (a new render handle follows).
    pub fn seek(&self, ms: u64) -> Result<(), CoreError> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(path) = inner.path.clone() else {
            return Err(err("player.no_track"));
        };
        self.start(&mut inner, path, ms).map(|_| ())
    }

    pub fn position_ms(&self) -> u64 {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let rate = inner.format.as_ref().map_or(0, |f| f.sample_rate);
        let frames = self.shared.frames_out.load(Ordering::Acquire);
        inner.base_ms
            + if rate == 0 {
                0
            } else {
                frames * 1000 / u64::from(rate)
            }
    }

    /// The track played to its end (the app moves on to the next one).
    pub fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::Acquire)
    }

    /// The equalizer (D-148): heard within `FILL_MS`.
    pub fn set_eq(&self, enabled: bool, preamp_db: f32, bands_db: Vec<f32>) {
        if let Ok(mut p) = self.shared.eq.lock() {
            *p = EqParams {
                enabled,
                preamp_db,
                bands_db,
            };
        }
        self.shared.eq_gen.fetch_add(1, Ordering::AcqRel);
    }

    /// How far ahead to decode: ~400 ms on screen (settings heard at once), more off screen or
    /// locked (nothing to hear quickly there, and a deeper buffer rides out a throttled thread).
    pub fn set_fill_ms(&self, ms: u32) {
        self.shared
            .fill_ms
            .store(ms.clamp(100, 1_500), Ordering::Relaxed);
    }

    /// Normalization gain for the open track, dB (`Library::norm_gain_db`; 0 — off).
    pub fn set_gain_db(&self, db: f32) {
        self.shared.gain_db.store(db.to_bits(), Ordering::Release);
        self.shared.gain_gen.fetch_add(1, Ordering::AcqRel);
    }

    /// A transition fade (D-148): this player's level goes to `target` (0…1) over `ms`, smooth
    /// per frame in the callback; `ms` 0 — at once.
    pub fn fade_to(&self, target: f32, ms: u32) {
        self.shared
            .fade_target
            .store(target.clamp(0.0, 1.0).to_bits(), Ordering::Release);
        self.shared.fade_ms.store(ms, Ordering::Release);
        self.shared.fade_gen.fetch_add(1, Ordering::AcqRel);
    }

    /// A visualiser feed (D-148): while it lives, the render callback copies what it plays
    /// aside. Drop it when the visualiser closes or the app leaves the screen.
    pub fn viz(self: Arc<Self>) -> Arc<VizFeed> {
        self.shared.viz_on.fetch_add(1, Ordering::AcqRel);
        Arc::new(VizFeed {
            player: self,
            state: Mutex::new(FeedState {
                tap: None,
                builder: FrameBuilder::new(),
                out: vec![0; FRAME],
            }),
        })
    }
}

/// Frames for the visualiser page — the PC's S-04 bytes (`sirtone_audio::vizframe`).
#[derive(uniffi::Object)]
pub struct VizFeed {
    player: Arc<Player>,
    state: Mutex<FeedState>,
}

struct FeedState {
    tap: Option<VizTap>,
    builder: FrameBuilder,
    out: Vec<u8>,
}

#[uniffi::export]
impl VizFeed {
    /// The next frame. `latency_ms` — what the system still holds before the sound is heard
    /// (`outputLatency + ioBufferDuration`; AirPods ≈ 200 ms), so picture and sound agree.
    pub fn frame(&self, latency_ms: u32) -> Vec<u8> {
        let shared = &self.player.shared;
        let mut st = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let st = &mut *st;
        if let Some(t) = shared
            .viz_tap
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            st.builder.set_rate(t.rate);
            st.tap = Some(t);
        }
        let mut rate = 48_000;
        if let Some(t) = st.tap.as_mut() {
            rate = t.rate;
            let n = t.consumer.slots() / t.channels.max(1) * t.channels.max(1);
            if let Ok(chunk) = t.consumer.read_chunk(n) {
                let (a, b) = chunk.as_slices();
                st.builder.push(a, t.channels);
                st.builder.push(b, t.channels);
                chunk.commit_all();
            }
        }
        let playing = !shared.paused.load(Ordering::Acquire);
        let latency = u64::from(latency_ms) * u64::from(rate) / 1000;
        st.builder.frame(
            &mut st.out,
            playing,
            self.player.position_ms(),
            0.0,
            0.0,
            latency,
        );
        st.out.clone()
    }
}

impl Drop for VizFeed {
    fn drop(&mut self) {
        self.player.shared.viz_on.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::stop_decoder(&mut inner, &self.shared);
    }
}

/// Real-time render: fills `frames × channels` interleaved `f32` at `out` and returns the frames
/// written from the track (the rest is silence). Pause and an empty ring give silence.
///
/// # Safety
/// `handle` comes from [`Player::take_render_handle`] and is used by one thread at a time (the
/// platform render thread) until [`sirtone_render_release`]; `out` points to
/// `frames * channels` writable floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sirtone_render(
    handle: u64,
    out: *mut f32,
    frames: u32,
    channels: u32,
) -> u32 {
    let len = frames as usize * channels as usize;
    if handle == 0 || out.is_null() || len == 0 {
        return 0;
    }
    // SAFETY: see the function contract.
    let (h, out) = unsafe {
        (
            &mut *(handle as usize as *mut RenderHandle),
            std::slice::from_raw_parts_mut(out, len),
        )
    };
    if h.shared.paused.load(Ordering::Acquire) {
        out.fill(0.0);
        return 0;
    }
    let volume = f32::from_bits(h.shared.volume.load(Ordering::Acquire));
    let fade_gen = h.shared.fade_gen.load(Ordering::Acquire);
    if fade_gen != h.fade_seen {
        h.fade_seen = fade_gen;
        h.fade_to = f32::from_bits(h.shared.fade_target.load(Ordering::Acquire));
        let ms = u64::from(h.shared.fade_ms.load(Ordering::Acquire));
        let frames = ms * u64::from(h.rate) / 1000;
        if frames == 0 {
            h.fade = h.fade_to;
            h.fade_step = 0.0;
        } else {
            h.fade_step = (h.fade_to - h.fade) / frames as f32;
        }
    }
    let n = h.consumer.slots().min(len) / channels as usize * channels as usize;
    if let Ok(chunk) = h.consumer.read_chunk(n) {
        let (a, b) = chunk.as_slices();
        // The visualiser's copy: only copying, whole frames, what fits (no wait, no allocation).
        if h.shared.viz_on.load(Ordering::Relaxed) > 0 {
            let room = h.viz.slots().min(n) / channels as usize * channels as usize;
            if let Ok(mut side) = h.viz.write_chunk_uninit(room) {
                let (x, y) = side.as_mut_slices();
                for (d, s) in x.iter_mut().chain(y.iter_mut()).zip(a.iter().chain(b)) {
                    d.write(*s);
                }
                // SAFETY: `room` ≤ `n` samples were available and all were written above.
                unsafe { side.commit_all() };
            }
        }
        let ch = channels as usize;
        for (i, (d, s)) in out.iter_mut().zip(a.iter().chain(b)).enumerate() {
            if i % ch == 0 && h.fade_step != 0.0 {
                h.fade += h.fade_step;
                let done = if h.fade_step > 0.0 {
                    h.fade >= h.fade_to
                } else {
                    h.fade <= h.fade_to
                };
                if done {
                    h.fade = h.fade_to;
                    h.fade_step = 0.0;
                }
            }
            *d = *s * volume * h.fade;
        }
        chunk.commit_all();
        h.shared.fade_now.store(h.fade.to_bits(), Ordering::Release);
    }
    out[n..].fill(0.0);
    let written = (n / channels as usize) as u32;
    h.shared
        .frames_out
        .fetch_add(u64::from(written), Ordering::AcqRel);
    if n == 0 && h.shared.decode_done.load(Ordering::Acquire) {
        h.shared.ended.store(true, Ordering::Release);
    }
    written
}

/// Gives back a render handle (after the node stopped using it).
///
/// # Safety
/// `handle` comes from [`Player::take_render_handle`] and is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sirtone_render_release(handle: u64) {
    if handle != 0 {
        // SAFETY: see the function contract.
        drop(unsafe { Box::from_raw(handle as usize as *mut RenderHandle) });
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 0.5 s 440 Hz stereo WAV (16-bit PCM, 44.1 kHz).
    pub(crate) fn wav(path: &std::path::Path) {
        let (rate, secs) = (44_100u32, 0.5f32);
        let n = (rate as f32 * secs) as u32;
        let mut data = Vec::with_capacity(n as usize * 4);
        for i in 0..n {
            let v =
                ((i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 12_000.0) as i16;
            data.extend_from_slice(&v.to_le_bytes());
            data.extend_from_slice(&v.to_le_bytes());
        }
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&rate.to_le_bytes());
        f.extend_from_slice(&(rate * 4).to_le_bytes());
        f.extend_from_slice(&4u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&(data.len() as u32).to_le_bytes());
        f.extend_from_slice(&data);
        std::fs::write(path, f).unwrap();
    }

    #[test]
    fn decodes_into_the_render_callback() {
        let path = std::env::temp_dir().join(format!("sirtone-player-{}.wav", std::process::id()));
        wav(&path);
        let p = Player::new();
        let f = p.open(path.to_string_lossy().into_owned()).unwrap();
        assert_eq!((f.sample_rate, f.channels), (44_100, 2));
        let h = p.take_render_handle();
        assert_ne!(h, 0);
        let mut out = vec![1.0f32; 512 * 2];

        // Paused: silence, nothing consumed.
        // SAFETY: the handle and buffer satisfy the contract.
        assert_eq!(unsafe { sirtone_render(h, out.as_mut_ptr(), 512, 2) }, 0);
        assert!(out.iter().all(|s| *s == 0.0));

        p.play();
        let (mut frames, mut peak) = (0u64, 0f32);
        for _ in 0..2_000 {
            // SAFETY: as above.
            let n = unsafe { sirtone_render(h, out.as_mut_ptr(), 512, 2) };
            frames += u64::from(n);
            peak = out.iter().fold(peak, |m, s| m.max(s.abs()));
            if p.ended() {
                break;
            }
            if n == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(p.ended(), "the track ends");
        assert_eq!(frames, 22_050, "every frame once, none lost");
        assert!(peak > 0.3, "sound, not silence: {peak}");
        assert!(
            (490..=510).contains(&p.position_ms()),
            "{}",
            p.position_ms()
        );

        // Seek: a new handle, playback from 250 ms.
        p.seek(250).unwrap();
        let h2 = p.take_render_handle();
        // SAFETY: the old handle is no longer used.
        unsafe { sirtone_render_release(h) };
        let mut got = 0u64;
        for _ in 0..2_000 {
            // SAFETY: as above.
            got += u64::from(unsafe { sirtone_render(h2, out.as_mut_ptr(), 512, 2) });
            if p.ended() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!((11_000..=11_100).contains(&got), "{got}");
        // SAFETY: done with it.
        unsafe { sirtone_render_release(h2) };
        drop(p);
        let _ = std::fs::remove_file(&path);
    }

    /// The visualiser sees what plays (D-148): the 440 Hz tone at full scale in the frames even
    /// at volume 0.1 (the copy is taken before the volume); closed, nothing is copied.
    #[test]
    fn the_visualiser_gets_what_plays() {
        let path =
            std::env::temp_dir().join(format!("sirtone-player-viz-{}.wav", std::process::id()));
        wav(&path);
        let p = Player::new();
        p.open(path.to_string_lossy().into_owned()).unwrap();
        let h = p.take_render_handle();
        p.set_volume(0.1);
        p.play();
        let feed = p.clone().viz();
        assert_eq!(p.shared.viz_on.load(Ordering::Relaxed), 1);
        let mut out = vec![0f32; 512 * 2];
        let mut rendered = 0u32;
        for _ in 0..2_000 {
            // SAFETY: the handle and buffer satisfy the contract.
            rendered += unsafe { sirtone_render(h, out.as_mut_ptr(), 512, 2) };
            if rendered >= 4_096 {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let f = feed.frame(0);
        assert_eq!(f.len(), sirtone_audio::vizframe::FRAME);
        assert_eq!(f[1], sirtone_audio::vizframe::FLAG_PLAYING);
        let l = &f[32 + 1024..32 + 2048];
        // 12 000 / 32 768 ≈ 0.37 → 128 ± 47; at volume 0.1 it would be ± 5.
        assert!(l.iter().any(|&x| x > 165) && l.iter().any(|&x| x < 91));
        drop(feed);
        assert_eq!(p.shared.viz_on.load(Ordering::Relaxed), 0);
        // SAFETY: done with it.
        unsafe { sirtone_render_release(h) };
        drop(p);
        let _ = std::fs::remove_file(&path);
    }

    /// Transitions and settings (D-148): the decoder stays about `FILL_MS` ahead (not the 2 s
    /// ring), `fade_to(0, 0)` silences at once, `fade_to(1, 100)` ramps per frame (one 256-frame
    /// buffer ≈ 6 % of 100 ms at 44.1 kHz, not a jump), and sound with EQ and gain set plays.
    #[test]
    fn fades_are_smooth_and_settings_apply() {
        let path =
            std::env::temp_dir().join(format!("sirtone-player-fade-{}.wav", std::process::id()));
        wav(&path);
        let p = Player::new();
        p.open(path.to_string_lossy().into_owned()).unwrap();
        let h = p.take_render_handle();
        p.set_eq(true, 0.0, vec![3.0; 10]);
        p.set_gain_db(-6.0);
        std::thread::sleep(Duration::from_millis(150));
        // SAFETY: the handle is alive and only read here.
        let ahead = unsafe { &*(h as usize as *const RenderHandle) }
            .consumer
            .slots();
        let fill = 44_100 * 2 * FILL_MS / 1000;
        assert!(ahead >= fill / 2 && ahead < RING_SAMPLES / 2, "{ahead}");

        p.play();
        let mut out = vec![1f32; 256 * 2];
        p.fade_to(0.0, 0);
        // SAFETY: the handle and buffer satisfy the contract.
        unsafe { sirtone_render(h, out.as_mut_ptr(), 256, 2) };
        assert!(out.iter().all(|x| *x == 0.0), "silent at once");
        p.fade_to(1.0, 100);
        // SAFETY: as above.
        unsafe { sirtone_render(h, out.as_mut_ptr(), 256, 2) };
        let first = f32::from_bits(p.shared.fade_now.load(Ordering::Relaxed));
        assert!((0.04..0.08).contains(&first), "a ramp, not a jump: {first}");
        let mut peak = 0f32;
        for _ in 0..30 {
            // SAFETY: as above.
            unsafe { sirtone_render(h, out.as_mut_ptr(), 256, 2) };
            peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            f32::from_bits(p.shared.fade_now.load(Ordering::Relaxed)),
            1.0
        );
        assert!(peak > 0.05, "sound with EQ and gain: {peak}");
        // SAFETY: done with it.
        unsafe { sirtone_render_release(h) };
        drop(p);
        let _ = std::fs::remove_file(&path);
    }
}
