//! sirtone-playback — Playback controller: queue, contexts, shuffle, repeat, sleep/focus execution.

pub mod automix;
pub mod queue;

pub use queue::{Queue, Repeat, ShuffleKey, Step, fewer_repeats};
