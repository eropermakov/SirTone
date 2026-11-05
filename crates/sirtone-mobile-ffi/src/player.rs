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
}

/// What the render callback owns. Handed to the app once as an opaque pointer.
pub struct RenderHandle {
    consumer: Consumer<f32>,
    shared: Arc<Shared>,
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
    loop {
        if shared.stop.load(Ordering::Acquire) {
            return;
        }
        if buf.is_empty() {
            match src.next_into(&mut buf) {
                Ok(true) => {}
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
        inner.handle = Some(Box::new(RenderHandle {
            consumer,
            shared: self.shared.clone(),
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
    let n = h.consumer.slots().min(len) / channels as usize * channels as usize;
    if let Ok(chunk) = h.consumer.read_chunk(n) {
        let (a, b) = chunk.as_slices();
        for (d, s) in out.iter_mut().zip(a.iter().chain(b)) {
            *d = *s * volume;
        }
        chunk.commit_all();
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
mod tests {
    use super::*;

    /// A 0.5 s 440 Hz stereo WAV (16-bit PCM, 44.1 kHz).
    fn wav(path: &std::path::Path) {
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
}
