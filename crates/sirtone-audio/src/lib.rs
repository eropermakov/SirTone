//! sirtone-audio — Decoding, resampling, DSP, mixer, automix, WASAPI output.
//!
//! Prototype 0.2 slice (T1.1–T1.4 minimal): [`decode::Source`] (symphonia + libopus),
//! [`output`] (WASAPI shared, event-driven, MMCSS) and the [`engine`] thread.

pub mod decode;
pub mod devices;
pub mod engine;
pub mod eq;
pub mod gapless;
pub mod latency;
pub mod limiter;
pub mod mf;
pub mod output;
pub mod pcm;
pub mod probe;
pub mod render;
pub mod viz;

pub use engine::{EngineCmd, EngineEvent, EngineHandle, PlayState, Snapshot, start};
