//! P-02 measurement (docs/08 §1): «play» command → first audible sample, on the real device.
//! The default endpoint is captured in loopback; each capture packet carries the QPC time of its
//! first frame, the command time is taken from the same clock. Played at volume 0.3 (−31 dB on the
//! cubic curve, a −18 dBFS tone → about −49 dBFS): barely audible, clear of the endpoint dither.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use sirtone_core::error::{AppError, ErrorCode};
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use crate::engine::{EngineCmd, EngineEvent};

/// Samples above this count as sound: ~10 LSB of a 16-bit endpoint, well above its dither.
const THRESHOLD: f32 = 3e-4;
/// Player volume during the measurement.
const VOLUME: f32 = 0.3;

/// Now on the QPC clock, in 100 ns units (the unit of capture timestamps).
fn qpc_100ns() -> u64 {
    let (mut c, mut f) = (0i64, 0i64);
    // SAFETY: plain out-parameters.
    unsafe {
        let _ = QueryPerformanceCounter(&mut c);
        let _ = QueryPerformanceFrequency(&mut f);
    }
    (c as u128 * 10_000_000 / f.max(1) as u128) as u64
}

/// Loopback of the default render endpoint: records when sound first appears after `arm`.
struct Probe {
    armed_at: Arc<AtomicU64>,
    heard_at: Arc<AtomicU64>,
    /// Sound in the 300 ms before arming (someone else is playing): the run is void.
    noisy: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Probe {
    fn start() -> Result<Self, AppError> {
        let armed_at = Arc::new(AtomicU64::new(0));
        let heard_at = Arc::new(AtomicU64::new(0));
        let noisy = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let (a, h, n, s) = (
            armed_at.clone(),
            heard_at.clone(),
            noisy.clone(),
            stop.clone(),
        );
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = (|| -> Result<(), String> {
                let _ = wasapi::initialize_mta().ok();
                let dev = DeviceEnumerator::new()
                    .and_then(|e| e.get_default_device(&Direction::Render))
                    .map_err(|e| e.to_string())?;
                let mut client = dev.get_iaudioclient().map_err(|e| e.to_string())?;
                let mix = client.get_mixformat().map_err(|e| e.to_string())?;
                let rate = mix.get_samplespersec();
                let ch = usize::from(mix.get_nchannels());
                let fmt = WaveFormat::new(32, 32, &SampleType::Float, rate as usize, ch, None);
                client
                    .initialize_client(
                        &fmt,
                        &Direction::Capture,
                        &StreamMode::PollingShared {
                            autoconvert: true,
                            buffer_duration_hns: 1_000_000,
                        },
                    )
                    .map_err(|e| e.to_string())?;
                let cap = client.get_audiocaptureclient().map_err(|e| e.to_string())?;
                client.start_stream().map_err(|e| e.to_string())?;
                let _ = ready_tx.send(Ok(()));
                let mut buf = vec![0u8; rate as usize * ch * 4];
                while !s.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(2));
                    while let Ok(Some(n_frames)) = cap.get_next_packet_size() {
                        if n_frames == 0 {
                            break;
                        }
                        let need = n_frames as usize * ch * 4;
                        if buf.len() < need {
                            buf.resize(need, 0);
                        }
                        let Ok((frames, info)) = cap.read_from_device(&mut buf[..need]) else {
                            break;
                        };
                        let armed = a.load(Ordering::Acquire);
                        if armed == 0 || h.load(Ordering::Relaxed) != 0 {
                            continue;
                        }
                        let first_loud = buf[..frames as usize * ch * 4]
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .position(|b| f32::from_le_bytes(*b).abs() > THRESHOLD);
                        let Some(i) = first_loud else { continue };
                        let t = info.timestamp + (i / ch) as u64 * 10_000_000 / u64::from(rate);
                        if t + 3_000_000 < armed {
                            continue; // older packet
                        }
                        if t < armed {
                            n.store(true, Ordering::Relaxed); // sound before the command
                        } else {
                            h.store(t, Ordering::Release);
                        }
                    }
                }
                let _ = client.stop_stream();
                Ok(())
            })();
            if let Err(e) = r {
                let _ = ready_tx.send(Err(e));
            }
        });
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| AppError::new(ErrorCode::DeviceUnavailable))?
            .map_err(|e| AppError::new(ErrorCode::DeviceUnavailable).with_details(e))?;
        Ok(Self {
            armed_at,
            heard_at,
            noisy,
            stop,
        })
    }

    /// Arms the detector at «now» and returns that time.
    fn arm(&self) -> u64 {
        self.heard_at.store(0, Ordering::Relaxed);
        self.noisy.store(false, Ordering::Relaxed);
        let t = qpc_100ns();
        self.armed_at.store(t, Ordering::Release);
        t
    }

    /// Milliseconds from arming to the first sound, `None` on timeout or noise.
    fn wait(&self, t0: u64, timeout: Duration) -> Option<f64> {
        let end = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < end {
            let h = self.heard_at.load(Ordering::Acquire);
            if h != 0 {
                return (!self.noisy.load(Ordering::Relaxed))
                    .then(|| (h.saturating_sub(t0)) as f64 / 10_000.0);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug, Default, Clone)]
pub struct Latency {
    /// Fresh engine, output not open yet (first «play» after launch).
    pub cold_ms: Vec<f64>,
    /// Output open, another track was playing and is paused.
    pub warm_ms: Vec<f64>,
}

/// `runs` cold and `runs` warm measurements with `file` (a tone works best).
pub fn measure(file: &Path, runs: usize) -> Result<Latency, AppError> {
    let probe = Probe::start()?;
    let mut out = Latency::default();
    for _ in 0..runs {
        // Cold: a new engine; its output opens on the first Load.
        let (tx, rx) = mpsc::channel();
        let engine = crate::engine::start(Box::new(move |ev| {
            let _ = tx.send(ev);
        }))?;
        engine.send(EngineCmd::SetVolume(VOLUME));
        std::thread::sleep(Duration::from_millis(400));
        let t0 = probe.arm();
        engine.send(EngineCmd::Load {
            path: file.to_path_buf(),
            autoplay: true,
            gain_db: 0.0,
        });
        if let Some(ms) = probe.wait(t0, Duration::from_secs(3)) {
            out.cold_ms.push(ms);
        }
        // Warm: pause, let it fall silent, then «play» another track.
        std::thread::sleep(Duration::from_millis(500));
        engine.send(EngineCmd::Pause);
        std::thread::sleep(Duration::from_millis(600));
        let t0 = probe.arm();
        engine.send(EngineCmd::Load {
            path: file.to_path_buf(),
            autoplay: true,
            gain_db: 0.0,
        });
        if let Some(ms) = probe.wait(t0, Duration::from_secs(3)) {
            out.warm_ms.push(ms);
        }
        engine.send(EngineCmd::Shutdown);
        while let Ok(ev) = rx.recv_timeout(Duration::from_millis(300)) {
            if let EngineEvent::Error { .. } = ev {
                break;
            }
        }
    }
    Ok(out)
}
