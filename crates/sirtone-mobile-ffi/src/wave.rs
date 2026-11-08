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
/// `mood`: upbeat, happy, calm, sad, and under «Ещё настройки» positive, negative; `language`:
/// ru, foreign, none). «Ещё настройки»: `character` (energetic, aggressive, calm), `daypart`
/// 0 dawn … 5 night, `weather` 0 clear … 5 storm, `season` auto / winter / spring / summer /
/// autumn / off. Empty — no filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct WaveTuning {
    pub activity: Vec<String>,
    pub frequency: Vec<String>,
    pub mood: Vec<String>,
    pub language: Vec<String>,
    pub character: Vec<String>,
    pub daypart: Option<u8>,
    pub weather: Option<u8>,
    pub season: Option<String>,
}

/// «Подходит N треков»; `relaxed` — too few, the wave widens the filters itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct TuningCount {
    pub tracks: u32,
    pub relaxed: bool,
}

fn query(t: &WaveTuning) -> Result<sirtone_wave::query::WaveQuery, CoreError> {
    use sirtone_core::wave::{Daypart, WeatherClass};
    use sirtone_wave::query::{Activity, Character, Freq, Lang, Mood, SeasonPick, WaveQuery};
    fn keys<T>(v: &[String], parse: fn(&str) -> Option<T>) -> Result<Vec<T>, CoreError> {
        v.iter()
            .map(|k| parse(k).ok_or_else(|| wave_err("wave.tuning")))
            .collect()
    }
    let pick = |i: Option<u8>, n: usize| -> Result<Option<usize>, CoreError> {
        match i {
            None => Ok(None),
            Some(i) if usize::from(i) < n => Ok(Some(usize::from(i))),
            Some(_) => Err(wave_err("wave.tuning")),
        }
    };
    let dayparts = [
        Daypart::Dawn,
        Daypart::Morning,
        Daypart::Day,
        Daypart::Evening,
        Daypart::Dusk,
        Daypart::Night,
    ];
    let weathers = [
        WeatherClass::Clear,
        WeatherClass::Cloudy,
        WeatherClass::Fog,
        WeatherClass::DrizzleRain,
        WeatherClass::Snow,
        WeatherClass::Storm,
    ];
    let season = match t.season.as_deref() {
        None | Some("auto") => SeasonPick::Auto,
        Some("off") => SeasonPick::Off,
        Some(s) => SeasonPick::Only(
            ["winter", "spring", "summer", "autumn"]
                .iter()
                .position(|x| *x == s)
                .ok_or_else(|| wave_err("wave.tuning"))? as u8,
        ),
    };
    Ok(WaveQuery {
        character: keys(&t.character, Character::from_key)?,
        daypart: pick(t.daypart, dayparts.len())?.map(|i| dayparts[i]),
        weather: pick(t.weather, weathers.len())?.map(|i| weathers[i]),
        season,
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
    context: Mutex<ContextBuilder>,
    /// «Моя волна» (the only profile the phone's context steers, as on the PC).
    my_wave: bool,
    /// What the person chose in «Настроить волну», the activity the phone's context suggests
    /// (D-149) and the influences (season, context, events; 1 — as the profile says).
    chosen: Mutex<Steer>,
}

#[derive(Default)]
struct Steer {
    query: sirtone_wave::query::WaveQuery,
    auto: Option<sirtone_wave::query::Activity>,
    /// Its key, for «Почему этот трек» («Вы в пути»).
    auto_key: Option<String>,
    influence: Option<[f32; 3]>,
}

impl Steer {
    /// The query the engine runs: the chosen one, the context's activity only when none is
    /// chosen (the PC's `with_activity`), the influences on top.
    fn effective(&self, my_wave: bool) -> sirtone_wave::query::WaveQuery {
        let mut q = self.query.clone();
        if my_wave
            && q.activity.is_empty()
            && let Some(a) = self.auto
        {
            q.activity = vec![a];
        }
        if let Some(i) = self.influence {
            q.influence = i;
        }
        q
    }
}

impl Wave {
    fn ctx(&self) -> std::sync::MutexGuard<'_, ContextBuilder> {
        self.context
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn steer(&self) -> std::sync::MutexGuard<'_, Steer> {
        self.chosen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The engine's reasons as codes.
    fn why_engine(&self, track_id: i64) -> Vec<String> {
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

#[uniffi::export]
impl Wave {
    /// «Моя волна» over `library`, learnt from its journal (D-128: unanalysed tracks take part).
    #[uniffi::constructor]
    pub fn start(library: Arc<Library>) -> Result<Arc<Self>, CoreError> {
        Self::start_profile(library, "my_wave".into())
    }

    /// Another built-in profile: `sleep` — «Засыпательная волна» (calmer and quieter towards the
    /// end, with the sleep timer the app starts), `seasonal`, `for_all`; anything else is
    /// «Моя волна».
    #[uniffi::constructor]
    pub fn start_profile(library: Arc<Library>, profile: String) -> Result<Arc<Self>, CoreError> {
        let profile = match profile.as_str() {
            "sleep" | "seasonal" | "for_all" => profile.as_str(),
            _ => "my_wave",
        };
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
        let cfg = Config::builtin_profile(profile).map_err(|_| wave_err("wave.profile"))?;
        // A different order each start, as on the PC.
        let seed = now as u64;
        Ok(Arc::new(Self {
            library,
            engine: Mutex::new(Engine::new(cfg, &tracks, &history, now, seed)),
            context: Mutex::new(ContextBuilder::default()),
            my_wave: profile == "my_wave",
            chosen: Mutex::new(Steer::default()),
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
        let ctx: ContextSnapshot = self.ctx().snapshot_at(&clock, now);
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
            context: self.ctx().snapshot_at(&clock, now),
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
        let ctx = self.ctx().snapshot_at(&clock, clock.now_utc_ms());
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
        let eff = {
            let mut st = self.steer();
            st.query = q;
            st.effective(self.my_wave)
        };
        let mut e = self
            .engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (n, relaxed) = e.query_count(&eff, now);
        e.set_query(eff, now);
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

    /// The activity the phone's context suggests (D-149; `context::auto_activity`), `None` —
    /// nothing or switched off. Only for «Моя волна» without a chosen activity. Returns whether
    /// the wave changed (the app then drops its queued picks and asks again).
    pub fn set_auto_activity(&self, activity: Option<String>) -> bool {
        use sirtone_wave::query::Activity;
        let a = activity.as_deref().and_then(Activity::from_key);
        let eff = {
            let mut st = self.steer();
            if st.auto == a {
                return false;
            }
            st.auto = a;
            st.auto_key = a.and(activity);
            if !self.my_wave || !st.query.activity.is_empty() {
                return false;
            }
            st.effective(true)
        };
        let now = PhoneClock { offset_min: 0 }.now_utc_ms();
        self.engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_query(eff, now);
        true
    }

    /// «Сила влияний» (S07): season, context, events, 0…2 (1 — as the profile says).
    pub fn set_influence(&self, season: f32, context: f32, events: f32) {
        let clamp = |v: f32| v.clamp(0.0, 2.0);
        let eff = {
            let mut st = self.steer();
            st.influence = Some([clamp(season), clamp(context), clamp(events)]);
            st.effective(self.my_wave)
        };
        let now = PhoneClock { offset_min: 0 }.now_utc_ms();
        self.engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .set_query(eff, now);
    }

    /// The weather where the phone is (Open-Meteo's answer to `context::weather_query`) and the
    /// place for the daypart by the sun. Returns the weather class (`clear`…`storm`) or `None`
    /// when the answer is unreadable. Only rounded coordinates are kept (D-149).
    pub fn set_weather(&self, json: Vec<u8>, lat: f64, lon: f64) -> Option<String> {
        let (lat, lon) = crate::context::coarse(lat, lon);
        let now = PhoneClock { offset_min: 0 }.now_utc_ms();
        let w = sirtone_wave::weather::parse(&json, lat, lon, now)?;
        let class = w
            .class()
            .map(sirtone_wave::weather::class_key)
            .map(str::to_owned);
        let mut c = self.ctx();
        c.coords = Some((lat, lon));
        c.southern = lat < 0.0;
        c.weather = Some(w);
        class
    }

    /// «Почему этот трек» (FR-LEARN-009): reason codes for the app to word — `liked`, `taste`,
    /// `session`, `new`, `forgotten`, `for_all`, `genre:<0..5>`, `context:<daypart>`,
    /// `season:<0..3>`.
    pub fn why(&self, track_id: i64) -> Vec<String> {
        // The phone's context first when it steers the wave now (D-149: the reason is named).
        let auto = {
            let st = self.steer();
            st.auto_key
                .clone()
                .filter(|_| self.my_wave && st.query.activity.is_empty())
                .map(|k| format!("auto:{k}"))
        };
        auto.into_iter().chain(self.why_engine(track_id)).collect()
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
        // The phone's context (D-149): an activity steers «Моя волна» and is named in «Почему»;
        // a chosen activity wins; the same value again changes nothing.
        assert!(wave.set_auto_activity(Some("road".into())));
        assert!(!wave.set_auto_activity(Some("road".into())));
        assert_eq!(
            wave.why(ids[0]).first().map(String::as_str),
            Some("auto:road")
        );
        wave.tune(WaveTuning {
            activity: vec!["work".into()],
            ..WaveTuning::default()
        })
        .unwrap();
        assert!(wave.why(ids[0]).iter().all(|w| !w.starts_with("auto:")));
        wave.tune(WaveTuning::default()).unwrap();
        assert!(wave.set_auto_activity(None));
        wave.set_influence(1.5, 0.0, 1.0);
        assert!(!wave.next(1, 0.5, 180).unwrap().is_empty());
        // An unreadable weather answer is ignored.
        assert_eq!(wave.set_weather(b"{}".to_vec(), 55.75, 37.62), None);
        let all_n = wave.tuning_count(WaveTuning::default()).unwrap().tracks;
        assert!(all_n >= ids.len() as u32);
        let liked = WaveTuning {
            frequency: vec!["loved".into()],
            ..WaveTuning::default()
        };
        let c = wave.tune(liked).unwrap();
        assert!(c.tracks <= all_n);
        assert!(!wave.next(1, 0.5, 180).unwrap().is_empty());
        let more = WaveTuning {
            character: vec!["calm".into()],
            daypart: Some(5),
            weather: Some(3),
            season: Some("winter".into()),
            ..WaveTuning::default()
        };
        assert!(wave.tuning_count(more).is_ok());
        assert!(
            wave.tuning_count(WaveTuning {
                daypart: Some(6),
                ..WaveTuning::default()
            })
            .is_err()
        );
        assert!(
            wave.tune(WaveTuning {
                mood: vec!["furious".into()],
                ..WaveTuning::default()
            })
            .is_err()
        );
        let sleep = Wave::start_profile(lib.clone(), "sleep".into()).unwrap();
        assert!(
            !sleep.next(2, 0.5, 180).unwrap().is_empty(),
            "the sleep wave plays too"
        );
        drop(sleep);
        drop(wave);
        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
