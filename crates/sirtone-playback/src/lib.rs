//! sirtone-playback — Playback controller: queue, contexts, shuffle, repeat, sleep/focus execution.

pub mod automix;
pub mod duck;
pub mod focus;
pub mod queue;
pub mod sleep;

pub use queue::{Queue, Repeat, ShuffleKey, Step, fewer_repeats};
