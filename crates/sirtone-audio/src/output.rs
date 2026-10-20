//! WASAPI shared, event-driven output (docs/04 §8, S-01). The render thread is registered with
//! MMCSS «Pro Audio» and only copies frames from the ring and applies the late multipliers
//! (volume with 8 ms smoothing, 15 ms pause ramp). No allocation, lock, log or IO in the loop;
//! everything it reports goes through atomics in [`Shared`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;

use sirtone_core::error::{AppError, ErrorCode};
use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

/// State shared between the engine thread and the render thread.
#[derive(Default)]
pub struct Shared {
    /// Frames taken from the ring and sent to the device (not silence).
    pub consumed: AtomicU64,
    /// Frames queued inside the device buffer (output latency).
    pub padding: AtomicU64,
    pub underruns: AtomicU64,
    /// Target linear gain (`f32` bits), already on the cubic curve.
    pub gain_bits: AtomicU32,
    /// Ducking target (T4.14, FR-PLAY-011), linear: 1 — none. A late multiplier: down over
    /// ≈ 150 ms (attack), back over ≈ 1.2 s (release), smoothed here.
    pub duck_bits: AtomicU32,
    /// Pause ramp target: true = audible.
    pub playing: AtomicBool,
    /// Ring may legitimately run dry (end of track, nothing loaded): no underrun.
    pub starving_ok: AtomicBool,
    /// Engine bumps `flush_req`; the render thread drops the ring and copies it to `flush_ack`.
    pub flush_req: AtomicU64,
    pub flush_ack: AtomicU64,
    pub stop: AtomicBool,
    /// Set by the render thread when the device failed (unplugged, invalidated).
    pub failed: AtomicBool,
    /// Live visualiser sources: while above zero, the render thread copies what it plays into
    /// the tap (T4.1). A counter, not a flag: a replaced source drops after its successor starts.
    pub viz_on: AtomicUsize,
    /// The tap of the current device, waiting for the visualiser thread to take it. Only the
    /// engine (on open) and the visualiser touch it — never the render thread.
    pub(crate) viz_tap: std::sync::Mutex<Option<crate::viz::Tap>>,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        let s = Self::default();
        s.gain_bits.store(1f32.to_bits(), Ordering::Relaxed);
        s.duck_bits.store(1f32.to_bits(), Ordering::Relaxed);
        s.starving_ok.store(true, Ordering::Relaxed);
        Arc::new(s)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceFormat {
    pub rate: u32,
    pub channels: usize,
}

pub struct Output {
    pub format: DeviceFormat,
    /// Endpoint id of the device this output plays to (for loss detection, T1.6).
    pub device_id: String,
    thread: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
}

/// Ring size: ~200 ms of headroom (docs/04 §1).
const RING_MS: usize = 200;
pub(crate) const PAUSE_RAMP_S: f32 = 0.015;
pub(crate) const VOLUME_SMOOTH_S: f32 = 0.008;
/// Ducking time constants (`04` §7: attack 150 ms, release 1.2 s ≈ 3 time constants).
const DUCK_ATTACK_S: f32 = 0.05;
const DUCK_RELEASE_S: f32 = 0.4;

/// Test hook: called on the render thread right before its real-time loop starts, so a test
/// allocator can assert that the loop never allocates (T1.3, `assert_no_alloc` style).
pub static RT_ENTER: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

impl Output {
    /// Opens the default render endpoint at its mix format. Returns the ring producer the engine
    /// fills. COM objects are thread-bound, so the device is opened on the render thread.
    pub fn open(shared: Arc<Shared>) -> Result<(Self, rtrb::Producer<f32>), AppError> {
        let (tx_fmt, rx_fmt) = mpsc::channel::<Result<(DeviceFormat, String), AppError>>();
        let (tx_ring, rx_ring) = mpsc::channel::<(rtrb::Consumer<f32>, rtrb::Producer<f32>)>();
        shared.stop.store(false, Ordering::Relaxed);
        shared.failed.store(false, Ordering::Relaxed);
        let rt_shared = shared.clone();
        let thread = std::thread::Builder::new()
            .name("sirtone-render".into())
            .spawn(move || {
                let _ = wasapi::initialize_mta().ok();
                match open_default() {
                    Ok((client, fmt, id)) => {
                        let format = DeviceFormat {
                            rate: fmt.get_samplespersec(),
                            channels: usize::from(fmt.get_nchannels()),
                        };
                        let _ = tx_fmt.send(Ok((format, id)));
                        let Ok((ring, tap)) = rx_ring.recv() else {
                            return;
                        };
                        if render_loop(client, fmt, ring, tap, &rt_shared).is_err() {
                            rt_shared.failed.store(true, Ordering::Release);
                        }
                    }
                    Err(e) => {
                        let _ = tx_fmt.send(Err(e));
                    }
                }
            })
            .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e))?;
        let (format, device_id) = rx_fmt
            .recv()
            .map_err(|e| AppError::new(ErrorCode::DeviceUnavailable).with_details(e))??;
        let (prod, cons) =
            rtrb::RingBuffer::<f32>::new(format.rate as usize * format.channels * RING_MS / 1000);
        // The visualiser's side ring (T4.1): filled only while `viz_on`.
        let (tap_prod, tap_cons) = rtrb::RingBuffer::<f32>::new(
            format.rate as usize * format.channels * crate::viz::TAP_MS / 1000,
        );
        if let Ok(mut slot) = shared.viz_tap.lock() {
            *slot = Some((tap_cons, format));
        }
        tx_ring
            .send((cons, tap_prod))
            .map_err(|e| AppError::new(ErrorCode::DeviceUnavailable).with_details(e))?;
        tracing::info!(target: "sirtone::audio", rate = format.rate, channels = format.channels, "output opened");
        Ok((
            Self {
                format,
                device_id,
                thread: Some(thread),
                shared,
            },
            prod,
        ))
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn device_err(e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::DeviceUnavailable)
        .retryable()
        .with_details(e)
}

fn open_default() -> Result<(AudioClient, WaveFormat, String), AppError> {
    let enumerator = DeviceEnumerator::new().map_err(device_err)?;
    let device = enumerator
        .get_default_device(&Direction::Render)
        .map_err(device_err)?;
    let id = device.get_id().unwrap_or_default();
    let mut client = device.get_iaudioclient().map_err(device_err)?;
    let mix = client.get_mixformat().map_err(device_err)?;
    let (def_period, _min) = client.get_device_period().map_err(device_err)?;
    let fmt = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        mix.get_samplespersec() as usize,
        usize::from(mix.get_nchannels()),
        None,
    );
    client
        .initialize_client(
            &fmt,
            &Direction::Render,
            &StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: def_period,
            },
        )
        .map_err(device_err)?;
    Ok((client, fmt, id))
}

fn register_mmcss() {
    let mut task_index = 0u32;
    // SAFETY: Win32 call with a static wide string and a valid out-pointer; the handle is
    // released by the OS when the thread exits.
    let _ = unsafe {
        windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW(
            windows::core::w!("Pro Audio"),
            &mut task_index,
        )
    };
}

fn render_loop(
    client: AudioClient,
    fmt: WaveFormat,
    mut ring: rtrb::Consumer<f32>,
    mut tap: rtrb::Producer<f32>,
    shared: &Shared,
) -> Result<(), AppError> {
    register_mmcss();
    let ch = usize::from(fmt.get_nchannels());
    let rate = fmt.get_samplespersec() as f32;
    let event = client.set_get_eventhandle().map_err(device_err)?;
    let render = client.get_audiorenderclient().map_err(device_err)?;
    let cap = client.get_buffer_size().map_err(device_err)? as usize;
    let mut bytes = vec![0u8; cap * ch * 4];
    let ramp_step = 1.0 / (PAUSE_RAMP_S * rate);
    let smooth = 1.0 - (-1.0 / (VOLUME_SMOOTH_S * rate)).exp();
    let mut pause_gain = 0f32;
    let mut gain = f32::from_bits(shared.gain_bits.load(Ordering::Relaxed));
    let duck_down = 1.0 - (-1.0 / (DUCK_ATTACK_S * rate)).exp();
    let duck_up = 1.0 - (-1.0 / (DUCK_RELEASE_S * rate)).exp();
    let mut duck = 1f32;

    let mut fill = |frames: usize, ring: &mut rtrb::Consumer<f32>| -> Result<(), AppError> {
        let req = shared.flush_req.load(Ordering::Acquire);
        if req != shared.flush_ack.load(Ordering::Relaxed) {
            let n = ring.slots();
            if let Ok(chunk) = ring.read_chunk(n) {
                chunk.commit_all();
            }
            shared.flush_ack.store(req, Ordering::Release);
        }
        let playing = shared.playing.load(Ordering::Relaxed);
        let target_gain = f32::from_bits(shared.gain_bits.load(Ordering::Relaxed));
        let duck_target = f32::from_bits(shared.duck_bits.load(Ordering::Relaxed));
        let out = &mut bytes[..frames * ch * 4];
        // Fully paused: keep the ring intact, write silence.
        let take = if !playing && pause_gain <= 0.0 {
            0
        } else {
            (ring.slots() / ch).min(frames)
        };
        let mut written = 0usize;
        // The visualiser's copy (T4.1): only copying, no allocation or lock.
        let viz = shared.viz_on.load(Ordering::Relaxed) > 0;
        if take > 0
            && let Ok(chunk) = ring.read_chunk(take * ch)
        {
            let (a, b) = chunk.as_slices();
            for (i, s) in a.iter().chain(b).enumerate() {
                if viz {
                    let _ = tap.push(*s);
                }
                if i % ch == 0 {
                    pause_gain = if playing {
                        (pause_gain + ramp_step).min(1.0)
                    } else {
                        (pause_gain - ramp_step).max(0.0)
                    };
                    gain += (target_gain - gain) * smooth;
                    duck += (duck_target - duck)
                        * if duck_target < duck {
                            duck_down
                        } else {
                            duck_up
                        };
                }
                let v = s * gain * pause_gain * duck;
                out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            chunk.commit_all();
            written = take;
            shared.consumed.fetch_add(take as u64, Ordering::Relaxed);
        }
        if written < frames {
            out[written * ch * 4..].fill(0);
            if playing && !shared.starving_ok.load(Ordering::Relaxed) {
                shared.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }
        render
            .write_to_device(frames, out, None)
            .map_err(device_err)?;
        if let Ok(p) = client.get_current_padding() {
            shared.padding.store(u64::from(p), Ordering::Relaxed);
        }
        Ok(())
    };

    let free = client.get_available_space_in_frames().map_err(device_err)? as usize;
    fill(free, &mut ring)?;
    // Everything below is the real-time loop: no allocation, lock, log or I/O (CLAUDE.md).
    if let Some(enter) = RT_ENTER.get() {
        enter();
    }
    client.start_stream().map_err(device_err)?;
    while !shared.stop.load(Ordering::Relaxed) {
        // 1 s without a device event = the endpoint is gone (unplugged/invalidated).
        event.wait_for_event(1000).map_err(device_err)?;
        let free = client.get_available_space_in_frames().map_err(device_err)? as usize;
        fill(free, &mut ring)?;
    }
    let _ = client.stop_stream();
    Ok(())
}
