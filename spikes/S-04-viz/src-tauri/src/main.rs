//! S-04: viz frames from Rust over a Tauri `Channel` at 60 Hz in the docs/04 §9 format:
//! 32-byte header (u8 version, u8 flags, u64 t_audio_ms, f32 beat_phase, f32 bpm, f32 bands[3])
//! + u8[1024] mono + u8[1024] L + u8[1024] R (offset 128). The header also carries a sequence
//! number and the Rust send time (µs) in the padding so the page can measure loss and latency.
#![windows_subsystem = "windows"]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

#[derive(Default)]
struct Running(Arc<AtomicBool>);

const FRAME: usize = 32 + 3 * 1024;
const PERIOD: Duration = Duration::from_nanos(16_666_667);

fn frame(seq: u32, t0: Instant, buf: &mut [u8]) {
    let t = t0.elapsed();
    let ms = t.as_millis() as u64;
    buf[0] = 1; // version
    buf[1] = 0; // flags
    buf[2..10].copy_from_slice(&ms.to_le_bytes());
    let bpm = 120.0f32;
    let beat_phase = ((t.as_secs_f32() * bpm / 60.0) % 1.0) as f32;
    buf[10..14].copy_from_slice(&beat_phase.to_le_bytes());
    buf[14..18].copy_from_slice(&bpm.to_le_bytes());
    // bands bass/mid/treb — pulsing with the beat
    let kick = (1.0 - beat_phase).powi(3);
    for (i, v) in [kick, 0.5, 0.3 + 0.2 * kick].iter().enumerate() {
        buf[18 + i * 4..22 + i * 4].copy_from_slice(&(*v as f32).to_le_bytes());
    }
    // spike-only: seq (u32) + send time µs (u32 wrap) in the last 8 header bytes
    buf[24..28].copy_from_slice(&seq.to_le_bytes());
    buf[28..32].copy_from_slice(&(t.as_micros() as u32).to_le_bytes());
    // waveform: 60 Hz kick + 440 Hz tone, L/R slightly different
    let secs = t.as_secs_f32();
    for i in 0..1024 {
        let s = secs + i as f32 / 44100.0;
        let l = 0.6 * kick * (2.0 * std::f32::consts::PI * 60.0 * s).sin() + 0.3 * (2.0 * std::f32::consts::PI * 440.0 * s).sin();
        let r = 0.6 * kick * (2.0 * std::f32::consts::PI * 60.0 * s).sin() + 0.3 * (2.0 * std::f32::consts::PI * 660.0 * s).sin();
        let q = |x: f32| (128.0 + (127.0 * x.clamp(-1.0, 1.0)).round()) as u8;
        buf[32 + i] = q((l + r) / 2.0);
        buf[32 + 1024 + i] = q(l);
        buf[32 + 2048 + i] = q(r);
    }
}

#[tauri::command]
fn start_stream(channel: Channel<InvokeResponseBody>, running: State<'_, Running>) {
    let flag = running.0.clone();
    flag.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        let t0 = Instant::now();
        let mut next = t0;
        let mut seq = 0u32;
        while flag.load(Ordering::SeqCst) {
            let mut buf = vec![0u8; FRAME];
            frame(seq, t0, &mut buf);
            if channel.send(InvokeResponseBody::Raw(buf)).is_err() {
                break;
            }
            seq += 1;
            next += PERIOD;
            // sleep most of the way, spin the rest: stable 60 Hz on Windows
            let now = Instant::now();
            if next > now + Duration::from_millis(2) {
                std::thread::sleep(next - now - Duration::from_millis(2));
            }
            while Instant::now() < next {
                std::hint::spin_loop();
            }
        }
    });
}

#[tauri::command]
fn stop_stream(running: State<'_, Running>) {
    running.0.store(false, Ordering::SeqCst);
}

fn main() {
    tauri::Builder::default()
        .manage(Running::default())
        .invoke_handler(tauri::generate_handler![start_stream, stop_stream])
        .run(tauri::generate_context!())
        .expect("run");
}
