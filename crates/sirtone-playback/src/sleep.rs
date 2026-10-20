//! Sleep timer (T4.21, FR-SLEEP-001/002/004/006, docs/14 §14; copies: Spotify/YouTube Music
//! presets and «до конца трека», Audible/Pocket Casts «shake to extend» → any action and
//! «+15 мин», foobar2000's lesson — fade in decibels, `01c` W-08).
//!
//! A pure state machine: the caller passes the time (`now_ms`, any monotonic clock) and, for
//! «до конца трека» / «через N треков», the moment the target track ends; nothing here reads a
//! clock. The volume goes 0 → `fade_db` linearly in dB over the fade, then `fade_db` →
//! `final_db` over the final seconds, then the caller pauses. A wake during the fade or the final
//! brings the volume back and freezes the progress; without «+15 мин» and without new input for
//! `wake_hold_ms` the fade continues from the very same level.

/// What the timer runs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepKind {
    Minutes(u32),
    /// The end of the playing track.
    EndOfTrack,
    /// The end of the N-th track counting the playing one as the first.
    Tracks(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SleepCfg {
    pub fade_ms: u64,
    pub final_ms: u64,
    pub fade_db: f32,
    pub final_db: f32,
    /// A wake holds the volume this long after the last input (Q-16: 2 min).
    pub wake_hold_ms: u64,
    /// «+15 мин».
    pub extend_ms: u64,
}

impl SleepCfg {
    /// `14` §14: fade 10 min 0 → −24 dB, final 30 s −24 → −60 dB, wake hold 2 min.
    pub fn new(fade_min: u32, final_s: u32) -> Self {
        Self {
            fade_ms: u64::from(fade_min) * 60_000,
            final_ms: u64::from(final_s) * 1000,
            fade_db: -24.0,
            final_db: -60.0,
            wake_hold_ms: 120_000,
            extend_ms: 15 * 60_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Counting down at full volume.
    Running,
    Fading,
    Final,
    /// Woken up during the fade or the final: full volume, «+15 мин» on offer.
    Woken,
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepEvent {
    /// The hold passed without «+15»: the fade continues where it stopped.
    Resumed,
    /// The end: pause now (and run the after-timer action).
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Woken {
    since_ms: u64,
    last_input_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SleepTimer {
    pub kind: SleepKind,
    cfg: SleepCfg,
    started_ms: u64,
    /// The moment of the pause (the final ends).
    end_ms: u64,
    /// Once a track-bound timer is woken and resumed, its end no longer follows the track.
    fixed: bool,
    woken: Option<Woken>,
    done: bool,
}

impl SleepTimer {
    /// `end_ms` — for the track kinds, when the target track ends (estimate, updated later by
    /// `set_end`); ignored for `Minutes`.
    pub fn start(kind: SleepKind, cfg: SleepCfg, now_ms: u64, end_ms: Option<u64>) -> Self {
        let end_ms = match kind {
            SleepKind::Minutes(m) => now_ms + u64::from(m) * 60_000,
            _ => end_ms.unwrap_or(now_ms),
        };
        Self {
            kind,
            cfg,
            started_ms: now_ms,
            end_ms,
            fixed: matches!(kind, SleepKind::Minutes(_)),
            woken: None,
            done: false,
        }
    }

    /// The target track's end moved (a seek, a skip, the next track began).
    pub fn set_end(&mut self, end_ms: u64) {
        if !self.fixed && !self.done {
            self.end_ms = end_ms;
        }
    }

    pub fn end_ms(&self) -> u64 {
        self.end_ms
    }

    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        self.end_ms.saturating_sub(now_ms)
    }

    /// Fade and final lengths for this timer: a timer shorter than both fades over its whole
    /// length (the final first).
    fn lens(&self) -> (u64, u64) {
        let total = self.end_ms.saturating_sub(self.started_ms);
        let fin = self.cfg.final_ms.min(total);
        let fade = self.cfg.fade_ms.min(total - fin);
        (fade, fin)
    }

    pub fn phase(&self, now_ms: u64) -> Phase {
        if self.done {
            return Phase::Done;
        }
        if self.woken.is_some() {
            return Phase::Woken;
        }
        let (fade, fin) = self.lens();
        let left = self.remaining_ms(now_ms);
        if left > fade + fin {
            Phase::Running
        } else if left > fin {
            Phase::Fading
        } else {
            Phase::Final
        }
    }

    /// The level the timer wants at `now_ms`, dB (0 — untouched).
    pub fn gain_db(&self, now_ms: u64) -> f32 {
        if self.woken.is_some() {
            return 0.0;
        }
        if self.done {
            return self.cfg.final_db;
        }
        self.curve_db(now_ms)
    }

    fn curve_db(&self, now_ms: u64) -> f32 {
        let (fade, fin) = self.lens();
        let left = self.remaining_ms(now_ms);
        if left > fade + fin {
            0.0
        } else if left > fin {
            let p = (fade + fin - left) as f32 / fade.max(1) as f32;
            self.cfg.fade_db * p
        } else {
            let q = if fin == 0 {
                1.0
            } else {
                (fin - left) as f32 / fin as f32
            };
            let from = if fade == 0 { 0.0 } else { self.cfg.fade_db };
            from + (self.cfg.final_db - from) * q
        }
    }

    /// t/T: how far through the whole timer, 0…1 (the sleep wave's energy curve).
    pub fn fraction(&self, now_ms: u64) -> f32 {
        let total = self.end_ms.saturating_sub(self.started_ms);
        if total == 0 {
            return 1.0;
        }
        (now_ms.saturating_sub(self.started_ms) as f32 / total as f32).clamp(0.0, 1.0)
    }

    /// 0…1: how far through the fade and the final («тёплый звук» glides with it).
    pub fn progress(&self, now_ms: u64) -> f32 {
        if self.woken.is_some() {
            return 0.0;
        }
        let (fade, fin) = self.lens();
        let span = fade + fin;
        if span == 0 {
            return 0.0;
        }
        let left = self.remaining_ms(now_ms).min(span);
        (span - left) as f32 / span as f32
    }

    /// Any action of the person (in the app, a hotkey, the tray, or input anywhere in the
    /// system). During the fade or the final it wakes the timer (`true` — show «+15 мин»);
    /// while woken it keeps the volume up.
    pub fn input(&mut self, now_ms: u64) -> bool {
        if self.done {
            return false;
        }
        if let Some(w) = &mut self.woken {
            w.last_input_ms = now_ms;
            return false;
        }
        if matches!(self.phase(now_ms), Phase::Fading | Phase::Final) {
            self.woken = Some(Woken {
                since_ms: now_ms,
                last_input_ms: now_ms,
            });
            return true;
        }
        false
    }

    /// «+15 мин»: the timer starts over to end 15 minutes from now (Pocket Casts restarts it).
    pub fn extend(&mut self, now_ms: u64) {
        if self.done {
            return;
        }
        self.woken = None;
        self.started_ms = now_ms;
        self.end_ms = now_ms + self.cfg.extend_ms;
        self.fixed = true;
    }

    /// Advance: a woken timer resumes after the hold; the end says pause.
    pub fn tick(&mut self, now_ms: u64) -> Option<SleepEvent> {
        if self.done {
            return None;
        }
        if let Some(w) = self.woken {
            if now_ms.saturating_sub(w.last_input_ms) < self.cfg.wake_hold_ms {
                return None;
            }
            // Continue from the very level of the wake: shift the end by the woken time.
            let left_at_wake = self.end_ms.saturating_sub(w.since_ms);
            let shift = now_ms - w.since_ms;
            self.started_ms += shift;
            self.end_ms = now_ms + left_at_wake;
            self.fixed = true;
            self.woken = None;
            return Some(SleepEvent::Resumed);
        }
        if now_ms >= self.end_ms {
            self.done = true;
            return Some(SleepEvent::Ended);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    fn cfg() -> SleepCfg {
        SleepCfg::new(10, 30)
    }

    #[test]
    fn the_fade_is_a_straight_line_in_db_then_the_final() {
        let t = SleepTimer::start(SleepKind::Minutes(30), cfg(), 0, None);
        let fade_start = 30 * MIN - 30_000 - 10 * MIN;
        assert_eq!(t.gain_db(fade_start - 1), 0.0);
        assert_eq!(t.phase(fade_start - 1), Phase::Running);
        for k in 0..=10 {
            let at = fade_start + k * MIN;
            let want = -24.0 * k as f32 / 10.0;
            assert!(
                (t.gain_db(at) - want).abs() < 1e-4,
                "{k}: {}",
                t.gain_db(at)
            );
        }
        assert_eq!(t.phase(fade_start + MIN), Phase::Fading);
        let fin = 30 * MIN - 30_000;
        assert!((t.gain_db(fin + 15_000) - -42.0).abs() < 1e-4);
        assert_eq!(t.phase(fin + 1), Phase::Final);
        assert!((t.gain_db(30 * MIN) - -60.0).abs() < 1e-4);
    }

    #[test]
    fn it_ends_once_at_the_end() {
        let mut t = SleepTimer::start(SleepKind::Minutes(15), cfg(), 1000, None);
        assert_eq!(t.tick(1000 + 15 * MIN - 1), None);
        assert_eq!(t.tick(1000 + 15 * MIN), Some(SleepEvent::Ended));
        assert_eq!(t.tick(1000 + 16 * MIN), None);
        assert_eq!(t.phase(1000 + 16 * MIN), Phase::Done);
    }

    #[test]
    fn a_wake_restores_and_two_quiet_minutes_resume_from_the_same_level() {
        let mut t = SleepTimer::start(SleepKind::Minutes(30), cfg(), 0, None);
        let at = 25 * MIN;
        let level = t.gain_db(at);
        assert!(level < -10.0);
        assert!(t.input(at), "the wake offers «+15»");
        assert_eq!(t.gain_db(at + 10), 0.0);
        assert_eq!(t.phase(at + 10), Phase::Woken);
        // More input keeps it awake.
        assert!(!t.input(at + 60_000));
        assert_eq!(t.tick(at + 60_000 + 119_000), None);
        let back = at + 60_000 + 120_000;
        assert_eq!(t.tick(back), Some(SleepEvent::Resumed));
        assert!(
            (t.gain_db(back) - level).abs() < 1e-3,
            "{} vs {level}",
            t.gain_db(back)
        );
        assert_eq!(t.remaining_ms(back), 30 * MIN - at);
    }

    #[test]
    fn input_before_the_fade_does_nothing() {
        let mut t = SleepTimer::start(SleepKind::Minutes(30), cfg(), 0, None);
        assert!(!t.input(5 * MIN));
        assert_eq!(t.phase(5 * MIN), Phase::Running);
    }

    #[test]
    fn plus_fifteen_restarts_the_fade_at_the_new_end() {
        let mut t = SleepTimer::start(SleepKind::Minutes(30), cfg(), 0, None);
        t.input(25 * MIN);
        t.extend(25 * MIN + 5_000);
        let now = 25 * MIN + 5_000;
        assert_eq!(t.phase(now), Phase::Running);
        assert_eq!(t.remaining_ms(now), 15 * MIN);
        assert_eq!(t.gain_db(now + 4 * MIN), 0.0);
        assert!((t.gain_db(now + 15 * MIN - 30_000) - -24.0).abs() < 1e-4);
    }

    #[test]
    fn a_short_timer_fades_over_its_whole_length() {
        let t = SleepTimer::start(SleepKind::Minutes(5), cfg(), 0, None);
        assert_eq!(t.phase(0), Phase::Fading);
        assert!(t.gain_db(1).abs() < 0.01);
        assert!((t.gain_db(5 * MIN - 30_000) - -24.0).abs() < 1e-4);
        assert!((t.gain_db(5 * MIN) - -60.0).abs() < 1e-4);
    }

    #[test]
    fn end_of_track_follows_the_track() {
        let mut t = SleepTimer::start(SleepKind::EndOfTrack, cfg(), 0, Some(3 * MIN));
        assert_eq!(t.remaining_ms(0), 3 * MIN);
        // A seek or a skip moves the end.
        t.set_end(4 * MIN);
        assert_eq!(t.remaining_ms(0), 4 * MIN);
        assert_eq!(t.tick(4 * MIN), Some(SleepEvent::Ended));
    }

    #[test]
    fn minutes_ignore_track_ends() {
        let mut t = SleepTimer::start(SleepKind::Minutes(30), cfg(), 0, None);
        t.set_end(MIN);
        assert_eq!(t.remaining_ms(0), 30 * MIN);
    }

    #[test]
    fn progress_goes_zero_to_one_over_fade_and_final() {
        let t = SleepTimer::start(SleepKind::Minutes(30), cfg(), 0, None);
        assert_eq!(t.progress(0), 0.0);
        assert!((t.progress(30 * MIN) - 1.0).abs() < 1e-6);
        let mid = 30 * MIN - (10 * MIN + 30_000) / 2;
        assert!((t.progress(mid) - 0.5).abs() < 1e-3);
    }
}
