//! S-01: WASAPI shared, event-driven output with MMCSS, a lock-free ring and an RT render loop that
//! only copies + applies an atomic gain (docs/04 §1, D-05). Verification without ears: our own
//! process is captured through application loopback and analysed (level, per-channel pitch,
//! discontinuities), and the render loop counts underruns.
//!
//! Usage: `cargo run --release -- <file> [--seconds N] [--gain-db G] [--stress]`

use std::collections::VecDeque;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

/// Shared between the engine, the RT thread and the UI side — atomics only.
#[derive(Default)]
struct Stats {
    underruns: AtomicU64,
    frames_rendered: AtomicU64,
    gain_bits: AtomicU32,
    max_loop_us: AtomicU64,
    eof: AtomicBool,
    stop: AtomicBool,
}

struct Args {
    file: PathBuf,
    seconds: f64,
    gain_db: f32,
    stress: bool,
    capture_pid: Option<u32>,
    sine_hz: Option<f64>,
}

fn parse_args() -> Args {
    let mut a = std::env::args().skip(1);
    let mut args = Args { file: PathBuf::new(), seconds: 6.0, gain_db: -30.0, stress: false, capture_pid: None, sine_hz: None };
    while let Some(x) = a.next() {
        match x.as_str() {
            "--seconds" => args.seconds = a.next().and_then(|v| v.parse().ok()).unwrap_or(6.0),
            "--gain-db" => args.gain_db = a.next().and_then(|v| v.parse().ok()).unwrap_or(-30.0),
            "--stress" => args.stress = true,
            "--capture-pid" => args.capture_pid = a.next().and_then(|v| v.parse().ok()),
            "--sine-hz" => args.sine_hz = a.next().and_then(|v| v.parse().ok()),
            _ => args.file = PathBuf::from(x),
        }
    }
    args
}

/// Engine thread: decode → convert channels → resample to the mix rate → ring. Allocations are fine
/// here (not RT).
fn engine(
    file: PathBuf,
    out_rate: u32,
    out_ch: usize,
    mut ring: rtrb::Producer<f32>,
    stats: Arc<Stats>,
) -> Result<(), String> {
    let src = File::open(&file).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = file.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| e.to_string())?;
    let track = format.default_track(TrackType::Audio).ok_or("no audio")?;
    let track_id = track.id;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("not audio")?.clone();
    let in_rate = params.sample_rate.ok_or("no rate")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| e.to_string())?;

    const CHUNK: usize = 1024;
    let mut resampler = if in_rate != out_rate {
        Some(Fft::<f32>::new(in_rate as usize, out_rate as usize, CHUNK, out_ch, FixedSync::Input).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let out_max = resampler.as_ref().map_or(CHUNK, |r| r.output_frames_max());
    let mut pending: Vec<f32> = Vec::new(); // decoded, channel-mapped, not yet resampled
    let mut out_buf = vec![0f32; out_max * out_ch];
    let mut dec_buf: Vec<f32> = Vec::new();
    let mut eof = false;

    let mut push = |data: &[f32], ring: &mut rtrb::Producer<f32>| {
        let mut off = 0;
        while off < data.len() {
            if stats.stop.load(Ordering::Relaxed) {
                return;
            }
            let n = ring.slots().min(data.len() - off);
            if n == 0 {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            if let Ok(chunk) = ring.write_chunk_uninit(n) {
                off += chunk.fill_from_iter(data[off..off + n].iter().copied());
            }
        }
    };

    loop {
        if stats.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        // Fill `pending` with at least one resampler chunk.
        while !eof && pending.len() < CHUNK * out_ch {
            match format.next_packet() {
                Ok(Some(p)) if p.track_id == track_id => {
                    if let Ok(audio) = decoder.decode(&p) {
                        let ch = audio.spec().channels().count();
                        dec_buf.resize(audio.samples_interleaved(), 0.0);
                        audio.copy_to_slice_interleaved(&mut dec_buf);
                        for frame in dec_buf.chunks_exact(ch) {
                            for c in 0..out_ch {
                                // mono → all, stereo → L/R, extra device channels → silence
                                let v = if ch == 1 { frame[0] } else if c < ch && c < 2 { frame[c] } else { 0.0 };
                                pending.push(v);
                            }
                        }
                    }
                }
                Ok(Some(_)) => {}
                _ => eof = true,
            }
        }
        if pending.is_empty() && eof {
            break;
        }
        match resampler.as_mut() {
            Some(r) => {
                let need = r.input_frames_next();
                if pending.len() < need * out_ch {
                    pending.resize(need * out_ch, 0.0); // tail: pad with silence
                }
                let input = InterleavedSlice::new(&pending[..need * out_ch], out_ch, need).map_err(|e| e.to_string())?;
                let mut output = InterleavedSlice::new_mut(&mut out_buf, out_ch, out_max).map_err(|e| e.to_string())?;
                let (_, written) = r.process_into_buffer(&input, &mut output, None).map_err(|e| e.to_string())?;
                pending.drain(..need * out_ch);
                let data: Vec<f32> = out_buf[..written * out_ch].to_vec();
                push(&data, &mut ring);
            }
            None => {
                let data = std::mem::take(&mut pending);
                push(&data, &mut ring);
            }
        }
    }
    stats.eof.store(true, Ordering::Release);
    Ok(())
}

/// RT render loop: MMCSS, wait for the device event, copy from the ring, apply the gain. No
/// allocation, no locks, no logging inside the loop.
fn render(
    mut client: AudioClient,
    fmt: WaveFormat,
    mut ring: rtrb::Consumer<f32>,
    stats: Arc<Stats>,
) -> Result<(), String> {
    let mut task_index = 0u32;
    // SAFETY: plain Win32 call with a static wide string and a valid out-pointer.
    let mmcss = unsafe {
        windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW(windows::core::w!("Pro Audio"), &mut task_index)
    };
    if mmcss.is_err() {
        eprintln!("MMCSS registration failed: {mmcss:?}");
    }
    let ch = fmt.get_nchannels() as usize;
    let block = fmt.get_blockalign() as usize;
    let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
    let render = client.get_audiorenderclient().map_err(|e| e.to_string())?;
    let cap = client.get_buffer_size().map_err(|e| e.to_string())? as usize;
    let mut bytes = vec![0u8; cap * block];

    let mut fill = |frames: usize, ring: &mut rtrb::Consumer<f32>| -> Result<(), String> {
        let gain = f32::from_bits(stats.gain_bits.load(Ordering::Relaxed));
        let want = frames * ch;
        let have = ring.slots().min(want);
        let out = &mut bytes[..frames * block];
        if let Ok(chunk) = ring.read_chunk(have) {
            let (a, b) = chunk.as_slices();
            for (i, s) in a.iter().chain(b).enumerate() {
                out[i * 4..i * 4 + 4].copy_from_slice(&(s * gain).to_le_bytes());
            }
            chunk.commit_all();
        }
        if have < want {
            out[have * 4..].fill(0);
            if !stats.eof.load(Ordering::Acquire) {
                stats.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }
        render.write_to_device(frames, out, None).map_err(|e| e.to_string())?;
        stats.frames_rendered.fetch_add(frames as u64, Ordering::Relaxed);
        Ok(())
    };

    // Pre-fill so playback starts with audio (MS «Rendering a Stream»): wait for the engine first.
    let t0 = Instant::now();
    while ring.slots() < cap * ch && t0.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(2));
    }
    let free = client.get_available_space_in_frames().map_err(|e| e.to_string())? as usize;
    fill(free, &mut ring)?;
    client.start_stream().map_err(|e| e.to_string())?;
    while !stats.stop.load(Ordering::Relaxed) {
        if event.wait_for_event(1000).is_err() {
            return Err("device event timeout (device lost?)".into());
        }
        let t = Instant::now();
        let free = client.get_available_space_in_frames().map_err(|e| e.to_string())? as usize;
        fill(free, &mut ring)?;
        stats.max_loop_us.fetch_max(t.elapsed().as_micros() as u64, Ordering::Relaxed);
    }
    client.stop_stream().map_err(|e| e.to_string())?;
    Ok(())
}

/// Captures this process's own audio (application loopback) for analysis.
fn capture_self(stats: Arc<Stats>, pid: u32) -> Result<(Vec<f32>, u32, usize), String> {
    wasapi::initialize_mta().ok().map_err(|e| e.to_string())?;
    let fmt = WaveFormat::new(32, 32, &SampleType::Float, 48000, 2, None);
    let mut client = AudioClient::new_application_loopback_client(pid, true).map_err(|e| e.to_string())?;
    client
        .initialize_client(&fmt, &Direction::Capture, &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 200_000 })
        .map_err(|e| e.to_string())?;
    let event = client.set_get_eventhandle().map_err(|e| e.to_string())?;
    let capture = client.get_audiocaptureclient().map_err(|e| e.to_string())?;
    let mut q: VecDeque<u8> = VecDeque::new();
    let mut discontinuities = 0usize;
    let mut packets = 0usize;
    client.start_stream().map_err(|e| e.to_string())?;
    while !stats.stop.load(Ordering::Relaxed) {
        if event.wait_for_event(500).is_err() {
            continue;
        }
        loop {
            let before = q.len();
            let info = capture.read_from_device_to_deque(&mut q).map_err(|e| e.to_string())?;
            if q.len() == before {
                break;
            }
            packets += 1;
            if info.flags.data_discontinuity && packets > 1 {
                discontinuities += 1;
            }
        }
    }
    client.stop_stream().ok();
    let bytes: Vec<u8> = q.into();
    let samples = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    Ok((samples, 48000, discontinuities))
}

/// Opens the default render endpoint in shared event mode at its mix rate/channel count, f32.
/// COM objects are thread-bound, so this runs on the render thread.
fn open_default() -> Result<(AudioClient, WaveFormat), String> {
    let enumerator = DeviceEnumerator::new().map_err(|e| e.to_string())?;
    let device = enumerator
        .get_default_device(&Direction::Render)
        .map_err(|e| format!("no default render device: {e}"))?;
    println!("default: {}", device.get_friendlyname().unwrap_or_default());
    let mut client = device.get_iaudioclient().map_err(|e| e.to_string())?;
    let mix = client.get_mixformat().map_err(|e| e.to_string())?;
    let (def_period, min_period) = client.get_device_period().map_err(|e| e.to_string())?;
    println!(
        "mix format: {} Hz, {} ch, {} bit {:?}; period default {:.1} ms, min {:.1} ms",
        mix.get_samplespersec(),
        mix.get_nchannels(),
        mix.get_bitspersample(),
        mix.get_subformat(),
        def_period as f64 / 1e4,
        min_period as f64 / 1e4
    );
    let fmt = WaveFormat::new(32, 32, &SampleType::Float, mix.get_samplespersec() as usize, mix.get_nchannels() as usize, None);
    client
        .initialize_client(&fmt, &Direction::Render, &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: def_period })
        .map_err(|e| e.to_string())?;
    println!("render buffer: {} frames", client.get_buffer_size().unwrap_or(0));
    Ok((client, fmt))
}

/// Goertzel power of `freq` in one channel of interleaved stereo.
fn goertzel(x: &[f32], ch: usize, which: usize, rate: f32, freq: f32) -> f64 {
    let w = 2.0 * std::f64::consts::PI * freq as f64 / rate as f64;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0f64, 0f64);
    let mut n = 0f64;
    for frame in x.chunks_exact(ch) {
        let s0 = frame[which] as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
        n += 1.0;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2) / (n * n).max(1.0)
}

fn main() {
    let args = parse_args();
    if let Some(pid) = args.capture_pid {
        // Verify another process (the app): capture its audio for --seconds and analyse.
        let stats = Arc::new(Stats::default());
        let st = stats.clone();
        let t = std::thread::spawn(move || capture_self(st, pid));
        std::thread::sleep(Duration::from_secs_f64(args.seconds));
        stats.stop.store(true, Ordering::Relaxed);
        report(t.join().expect("capture join"), args.sine_hz);
        return;
    }
    wasapi::initialize_mta().ok().expect("COM");
    let enumerator = DeviceEnumerator::new().expect("enumerator");
    println!("render endpoints:");
    if let Ok(coll) = enumerator.get_device_collection(&Direction::Render) {
        for dev in &coll {
            if let Ok(d) = dev {
                println!("  - {}", d.get_friendlyname().unwrap_or_default());
            }
        }
    }
    let (tx_fmt, rx_fmt) = std::sync::mpsc::channel::<Result<(u32, usize), String>>();
    let (tx_cons, rx_cons) = std::sync::mpsc::channel::<rtrb::Consumer<f32>>();
    let stats = Arc::new(Stats::default());
    stats.gain_bits.store(10f32.powf(args.gain_db / 20.0).to_bits(), Ordering::Relaxed);
    let rt_stats = stats.clone();
    let render_t = std::thread::Builder::new()
        .name("sirtone-render".into())
        .spawn(move || {
            wasapi::initialize_mta().ok().ok();
            match open_default() {
                Ok((client, fmt)) => {
                    let _ = tx_fmt.send(Ok((fmt.get_samplespersec(), fmt.get_nchannels() as usize)));
                    let cons = rx_cons.recv().map_err(|e| e.to_string())?;
                    render(client, fmt, cons, rt_stats)
                }
                Err(e) => {
                    let _ = tx_fmt.send(Err(e.clone()));
                    Err(e)
                }
            }
        })
        .expect("render thread");
    let (rate, ch) = match rx_fmt.recv().expect("render thread reply") {
        Ok(f) => f,
        Err(e) => {
            println!("{e} (graceful exit)");
            return;
        }
    };
    let (prod, cons) = rtrb::RingBuffer::<f32>::new(rate as usize * ch / 5); // 200 ms
    let cap_stats = stats.clone();
    let capture = std::thread::spawn(move || capture_self(cap_stats, std::process::id()));
    std::thread::sleep(Duration::from_millis(200));

    let eng_stats = stats.clone();
    let file = args.file.clone();
    let engine_t = std::thread::spawn(move || engine(file, rate, ch, prod, eng_stats));
    tx_cons.send(cons).expect("send ring");

    let stress_stop = Arc::new(AtomicBool::new(false));
    let mut stressors = Vec::new();
    if args.stress {
        let n = std::thread::available_parallelism().map_or(8, |n| n.get());
        for _ in 0..n {
            let stop = stress_stop.clone();
            stressors.push(std::thread::spawn(move || {
                let mut x = 1.0001f64;
                while !stop.load(Ordering::Relaxed) {
                    for _ in 0..100_000 {
                        x = (x * 1.000_000_1).sin().abs() + 1.0;
                    }
                }
                x
            }));
        }
        println!("stress: {n} busy threads on all cores");
    }

    let t0 = Instant::now();
    while t0.elapsed().as_secs_f64() < args.seconds && !stats.eof.load(Ordering::Acquire) {
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(Duration::from_millis(300)); // let the tail drain
    stats.stop.store(true, Ordering::Relaxed);
    stress_stop.store(true, Ordering::Relaxed);
    for s in stressors {
        let _ = s.join();
    }
    let render_res = render_t.join().expect("render join");
    let engine_res = engine_t.join().expect("engine join");
    let played = stats.frames_rendered.load(Ordering::Relaxed) as f64 / rate as f64;
    println!(
        "render: {:?}, engine: {:?}, played {:.2} s, underruns {}, max loop {} µs",
        render_res,
        engine_res,
        played,
        stats.underruns.load(Ordering::Relaxed),
        stats.max_loop_us.load(Ordering::Relaxed)
    );

    report(capture.join().expect("capture join"), None);
}

fn report(result: Result<(Vec<f32>, u32, usize), String>, sine_hz: Option<f64>) {
    match result {
        Ok((cap, crate_rate, disc)) => {
            let frames = cap.len() / 2;
            // Ignore leading silence before the first non-zero sample.
            let start = cap.iter().position(|s| s.abs() > 1e-6).unwrap_or(cap.len()) & !1;
            let body = &cap[start..];
            let rms = (body.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / body.len().max(1) as f64).sqrt();
            let silent_runs = body
                .chunks_exact(2 * 480)
                .filter(|w| w.iter().all(|s| s.abs() < 1e-7))
                .count();
            println!(
                "loopback: {:.2} s captured, signal {:.2} s, RMS {:.1} dBFS, 10 ms silent blocks inside signal: {}, discontinuity flags: {}",
                frames as f64 / crate_rate as f64,
                body.len() as f64 / 2.0 / crate_rate as f64,
                20.0 * rms.max(1e-12).log10(),
                silent_runs,
                disc
            );
            // Click detector for the 440 Hz (L) / 660 Hz (R) test tone: a pure sine obeys
            // x[n+1] = 2·cos(w)·x[n] − x[n−1]; any dropped/duplicated samples break it.
            let clicks = |which: usize, f: f64| -> (usize, f64) {
                let x: Vec<f64> = body.chunks_exact(2).map(|fr| fr[which] as f64).collect();
                let trim = (crate_rate as usize) / 10;
                if x.len() < 3 * trim {
                    return (0, 0.0);
                }
                let x = &x[trim..x.len() - trim];
                let peak = x.iter().fold(0f64, |m, v| m.max(v.abs())).max(1e-9);
                let c = 2.0 * (2.0 * std::f64::consts::PI * f / crate_rate as f64).cos();
                let mut n = 0;
                let mut worst = 0f64;
                for i in 1..x.len() - 1 {
                    let r = (x[i + 1] + x[i - 1] - c * x[i]).abs() / peak;
                    worst = worst.max(r);
                    if r > 0.02 {
                        n += 1;
                    }
                }
                (n, worst)
            };
            let (cl, wl) = clicks(0, 440.0);
            let (cr, wr) = clicks(1, 660.0);
            println!("  clicks (tone-10s): L {cl} (worst {wl:.3}), R {cr} (worst {wr:.3})");
            if let Some(f) = sine_hz {
                let (a, wa) = clicks(0, f);
                let (b, wb) = clicks(1, f);
                println!("  clicks ({f} Hz): L {a} (worst {wa:.3}), R {b} (worst {wb:.3})");
            }
            for (name, f) in [("440 Hz", 440.0), ("660 Hz", 660.0), ("1 kHz", 1000.0)] {
                println!(
                    "  {name}: L {:.1} dB, R {:.1} dB",
                    10.0 * goertzel(body, 2, 0, crate_rate as f32, f).max(1e-20).log10(),
                    10.0 * goertzel(body, 2, 1, crate_rate as f32, f).max(1e-20).log10()
                );
            }
        }
        Err(e) => println!("loopback capture failed: {e}"),
    }
}
