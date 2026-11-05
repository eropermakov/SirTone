//! sirtone-audio — Decoding, resampling, DSP, mixer, automix, WASAPI output.
//!
//! Prototype 0.2 slice (T1.1–T1.4 minimal): [`decode::Source`] (symphonia + libopus),
//! [`output`] (WASAPI shared, event-driven, MMCSS) and the [`engine`] thread.

pub mod decode;
#[cfg(windows)]
pub mod devices;
#[cfg(windows)]
pub mod engine;
pub mod eq;
pub mod gapless;
#[cfg(windows)]
pub mod latency;
pub mod limiter;
#[cfg(windows)]
pub mod mf;
#[cfg(windows)]
pub mod output;
pub mod pcm;
pub mod probe;
#[cfg(windows)]
pub mod render;
pub mod sfx;
pub mod stretch;
#[cfg(windows)]
mod swap;
#[cfg(windows)]
pub mod viz;
pub mod warm;

#[cfg(windows)]
pub use engine::{EngineCmd, EngineEvent, EngineHandle, PlayState, Snapshot, start};
