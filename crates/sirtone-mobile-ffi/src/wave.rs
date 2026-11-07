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

/// «Настроить волну» on the phone (MR S07; the compact four blocks of D-130): keys as on the PC
/// (`activity`: workout, sleep, work, road, rest, party; `frequency`: loved, often, forgotten;
/// `mood`: upbeat, happy, calm, sad; `language`: ru, foreign, none). Empty — no filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct WaveTuning {
    pub activity: Vec<String>,
    pub frequency: Vec<String>,
    pub mood: Vec<String>,
    pub language: Vec<String>,
}

/// «Подходит N треков»; `relaxed` — too few, the wave widens the filters itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct TuningCount {
    pub tracks: u32,
    pub relaxed: bool,
}

fn query(t: &WaveTuning) -> Result<sirtone_wave::query::WaveQuery, CoreError> {
    use sirtone_wave::query::{Activity, Freq, Lang, Mood, WaveQuery};
    fn keys<T>(v: &[String], parse: fn(&str) -> Option<T>) -> Result<Vec<T>, CoreError> {
        v.iter()
            .map(|k| parse(k).ok_or_else(|| wave_err("wave.tuning")))
            .collect()
    }
    Ok(WaveQuery {
        activity: keys(&t.activity, Activity::from_key)?,
        frequency: keys(&t.frequency, Freq::from_key)?,
        mood: keys(&t.mood, Mood::from_key)?,
        language: keys(&t.language, Lang::from_key)?,
        ..WaveQuery::default()
    })
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

    /// The player's journal row (the wave learns from it, `sirtone-transfer` carries it to the
    /// PC): `played_ms` heard, `skipped` when moved on early, `from_wave` when the wave chose it.
    /// The engine moves on too (as [`Self::played`] / [`Self::skipped`]).
    pub fn listened(
        &self,
        track_id: i64,
        played_ms: u64,
        duration_ms: u64,
        skipped: bool,
        from_wave: bool,
        utc_offset_min: i32,
    ) -> Result<(), CoreError> {
        use sirtone_core::wave::{EndReason, PlayEvent, PlaySource};
        let clock = PhoneClock {
            offset_min: utc_offset_min,
        };
        let now = clock.now_utc_ms();
        let event = PlayEvent {
            ts: now - played_ms as i64,
            track_id,
            played_ms,
            duration_ms,
            end_reason: if skipped {
                EndReason::Skip
            } else {
                EndReason::Finished
            },
            source: if from_wave {
                PlaySource::Wave
            } else {
                PlaySource::Collection
            },
            profile: from_wave.then(|| "my_wave".to_string()),
            context: self.context.snapshot_at(&clock, now),
            event_id: None,
            novelty: None,
            category: None,
        };
        self.library
            .with(|c| sirtone_library::wave::record_play(c, &event))?;
        if skipped {
            self.skipped(track_id);
        } else {
            self.played(track_id);
        }
        Ok(())
    }

    /// «Не сейчас»: a minus in this moment's context bucket only (D-77); the app moves on.
    pub fn not_now(&self, track_id: i64, utc_offset_min: i32) -> Result<(), CoreError> {
        let clock = PhoneClock {
            offset_min: utc_offset_min,
        };
        let ctx = self.context.snapshot_at(&clock, clock.now_utc_ms());
        let key = sirtone_wave::context::bucket_key(&ctx);
        self.library
            .with(|c| sirtone_library::feedback::not_now(c, track_id, &key))?;
        self.skipped(track_id);
        Ok(())
    }

    /// «Настроить волну»: the filters for this session, at once (the app drops the wave's queued
    /// picks and asks again). Returns what matches now.
    pub fn tune(&self, tuning: WaveTuning) -> Result<TuningCount, CoreError> {
        let q = query(&tuning)?;
        let now = PhoneClock { offset_min: 0 }.now_utc_ms();
        let mut e = self
            .engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (n, relaxed) = e.query_count(&q, now);
        e.set_query(q, now);
        Ok(TuningCount {
            tracks: u32::try_from(n).unwrap_or(u32::MAX),
            relaxed,
        })
    }

    /// The live «Подходит N треков» while choosing, without changing the wave.
    pub fn tuning_count(&self, tuning: WaveTuning) -> Result<TuningCount, CoreError> {
        let q = query(&tuning)?;
        let now = PhoneClock { offset_min: 0 }.now_utc_ms();
        let (n, relaxed) = self
            .engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .query_count(&q, now);
        Ok(TuningCount {
            tracks: u32::try_from(n).unwrap_or(u32::MAX),
            relaxed,
        })
    }

    /// «Почему этот трек» (FR-LEARN-009): reason codes for the app to word — `liked`, `taste`,
    /// `session`, `new`, `forgotten`, `for_all`, `genre:<0..5>`, `context:<daypart>`,
    /// `season:<0..3>`.
    pub fn why(&self, track_id: i64) -> Vec<String> {
        use sirtone_wave::engine::Why;
        self.engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .why(track_id)
            .into_iter()
            .map(|w| match w {
                Why::Liked => "liked".to_string(),
                Why::Genre(g) => format!("genre:{g}"),
                Why::Taste => "taste".to_string(),
                Why::Context(d) => format!("context:{d}"),
                Why::Session => "session".to_string(),
                Why::NewForYou => "new".to_string(),
                Why::Forgotten => "forgotten".to_string(),
                Why::Season(s) => format!("season:{s}"),
                Why::ForAll => "for_all".to_string(),
            })
            .collect()
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
        // The journal grows by the player's rows; «Не сейчас» and «Почему» answer.
        wave.listened(ids[0], 150_000, 200_000, false, true, 180)
            .unwrap();
        wave.listened(ids[0], 5_000, 200_000, true, false, 180)
            .unwrap();
        let rows: i64 = lib
            .with(|c| c.query_row("SELECT count(*) FROM play_event", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(rows, 2);
        wave.not_now(ids[0], 180).unwrap();
        let _ = wave.why(ids[0]);
        // «Настроить волну»: no filter counts everything, a filter narrows or widens itself,
        // an unknown key is refused, and the wave keeps playing after tuning.
        let all_n = wave.tuning_count(WaveTuning::default()).unwrap().tracks;
        assert!(all_n >= ids.len() as u32);
        let liked = WaveTuning {
            frequency: vec!["loved".into()],
            ..WaveTuning::default()
        };
        let c = wave.tune(liked).unwrap();
        assert!(c.tracks <= all_n);
        assert!(!wave.next(1, 0.5, 180).unwrap().is_empty());
        assert!(
            wave.tune(WaveTuning {
                mood: vec!["furious".into()],
                ..WaveTuning::default()
            })
            .is_err()
        );
        drop(wave);
        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
