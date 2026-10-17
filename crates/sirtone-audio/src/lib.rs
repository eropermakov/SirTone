//! sirtone-audio — Decoding, resampling, DSP, mixer, automix, WASAPI output.
//!
//! Prototype 0.2 slice (T1.1–T1.4 minimal): [`decode::Source`] (symphonia + libopus),
//! [`output`] (WASAPI shared, event-driven, MMCSS) and the [`engine`] thread.

pub mod decode;
pub mod devices;
pub mod engine;
pub mod gapless;
pub mod latency;
pub mod mf;
pub mod output;
pub mod probe;
pub mod render;

pub use engine::{EngineCmd, EngineEvent, EngineHandle, PlayState, Snapshot, start};
