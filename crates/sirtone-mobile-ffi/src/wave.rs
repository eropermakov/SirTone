//! «Моя волна» on the phone (M5, F07): the same engine as on the PC over the phone's library and
//! journal. The phone passes its local UTC offset (iOS `TimeZone.current`, Android `ZoneId`):
//! the core has no time-zone database of its own and the platform layer is not built here.

use std::sync::{Arc, Mutex};

use sirtone_core::wave::{Clock, ContextSnapshot};
use sirtone_wave::context::ContextBuilder;
use sirtone_wave::engine::{Config, Engine};
use sirtone_wave::learn::History;

use crate::CoreError;
use crate::library::Library;

/// Clock of the phone: system time and the offset the app reports.
struct PhoneClock {
    offset_min: i32,
}

impl Clock for PhoneClock {
    fn now_utc_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64)
    }

    fn local_offset_min(&self, _utc_ms: i64) -> i32 {
        self.offset_min
    }
}

fn wave_err(code: &str) -> CoreError {
    CoreError::Failed { code: code.into() }
}

#[derive(uniffi::Object)]
pub struct Wave {
    library: Arc<Library>,
    engine: Mutex<Engine>,
    context: ContextBuilder,
}

#[uniffi::export]
impl Wave {
    /// «Моя волна» over `library`, learnt from its journal (D-128: unanalysed tracks take part).
    #[uniffi::constructor]
    pub fn start(library: Arc<Library>) -> Result<Arc<Self>, CoreError> {
        let now = PhoneClock { offset_min: 0 }.now_utc_ms();
        let (tracks, history) = library.with(|c| {
            use sirtone_library::wave as w;
            Ok::<_, sirtone_library::LibraryError>((
                w::catalog(c)?,
                History {
                    events: w::plays_since(c, 0)?,
                    feedback: w::feedback_since(c, 0)?,
                },
            ))
        })?;
        let cfg = Config::builtin_profile("my_wave").map_err(|_| wave_err("wave.profile"))?;
        // A different order each start, as on the PC.
        let seed = now as u64;
        Ok(Arc::new(Self {
            library,
            engine: Mutex::new(Engine::new(cfg, &tracks, &history, now, seed)),
            context: ContextBuilder::default(),
        }))
    }

    /// The next `count` track ids for this moment. `novelty` — «Знакомое ↔ Неожиданное», 0…1.
    pub fn next(
        &self,
        count: u32,
        novelty: f32,
        utc_offset_min: i32,
    ) -> Result<Vec<i64>, CoreError> {
        let clock = PhoneClock {
            offset_min: utc_offset_min,
        };
        let now = clock.now_utc_ms();
        let ctx: ContextSnapshot = self.context.snapshot_at(&clock, now);
        let shelf = self
            .library
            .with(|c| sirtone_library::wave::not_now(c, now))?;
        let mut e = self
            .engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(e.next(
            &ctx,
            &shelf,
            novelty.clamp(0.0, 1.0),
            count.max(1) as usize,
            now,
        ))
    }

    /// The track was heard (the engine moves on; the journal row is written by the player).
    pub fn played(&self, track_id: i64) {
        let now = PhoneClock { offset_min: 0 }.now_utc_ms();
        self.engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .played(track_id, now);
    }

    /// The track was skipped: the session leans away from it right away.
    pub fn skipped(&self, track_id: i64) {
        let now = PhoneClock { offset_min: 0 }.now_utc_ms();
        self.engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .skipped(track_id, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wave_plays_from_the_library() {
        let (lib, dir) = crate::library::tests::sample();
        let wave = Wave::start(lib.clone()).unwrap();
        let ids = wave.next(3, 0.5, 180).unwrap();
        assert!(
            !ids.is_empty(),
            "the wave starts with unanalysed tracks (D-128)"
        );
        let all: Vec<i64> = lib
            .tracks(None, 10)
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.id)
            .collect();
        assert!(ids.iter().all(|id| all.contains(id)));
        wave.played(ids[0]);
        wave.skipped(ids[0]);
        assert!(!wave.next(1, 0.5, 180).unwrap().is_empty());
        drop(wave);
        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
