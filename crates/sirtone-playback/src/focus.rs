//! Focus cycles (T4.24, FR-FOCUS-001/002, docs/14 §15; copies: the Pomodoro method and
//! Pomofocus — 25/5, a long 15 after 4 cycles; Windows Clock — separate soft sounds at the end of
//! work and of a break, a day's counter, `01c` W-09).
//!
//! A pure state machine: the caller passes the time and the local day; nothing here reads a
//! clock. Pause freezes the segment; «пропустить» ends it at once.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusCfg {
    pub work_ms: u64,
    pub break_ms: u64,
    pub long_break_ms: u64,
    /// Work segments before a long break.
    pub cycles_before_long: u32,
}

impl FocusCfg {
    pub fn minutes(work: u32, brk: u32, long: u32, cycles: u32) -> Self {
        Self {
            work_ms: u64::from(work) * 60_000,
            break_ms: u64::from(brk) * 60_000,
            long_break_ms: u64::from(long) * 60_000,
            cycles_before_long: cycles.max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    Work,
    Break,
    LongBreak,
}

/// What a tick reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusEvent {
    /// A work segment ended (`focus_work_end` sound, the break's music); the day's count grew.
    WorkEnded,
    /// A break ended (`focus_break_end`, work music again).
    BreakEnded,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FocusCycle {
    cfg: FocusCfg,
    pub segment: Segment,
    /// When the current segment ends (meaningless while paused).
    end_ms: u64,
    /// Paused: the time left in the segment.
    paused_left: Option<u64>,
    /// Work segments done since the last long break.
    streak: u32,
    /// Work segments done today, and which day that is (local days since the epoch).
    today: u32,
    day: i32,
}

impl FocusCycle {
    /// Starts with work. `done_today` — the count already made today (kept by the caller).
    pub fn start(cfg: FocusCfg, now_ms: u64, day: i32, done_today: u32) -> Self {
        Self {
            cfg,
            segment: Segment::Work,
            end_ms: now_ms + cfg.work_ms,
            paused_left: None,
            streak: 0,
            today: done_today,
            day,
        }
    }

    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        self.paused_left
            .unwrap_or_else(|| self.end_ms.saturating_sub(now_ms))
    }

    /// When the segment ends (for packing tracks into it).
    pub fn end_ms(&self) -> Option<u64> {
        self.paused_left.is_none().then_some(self.end_ms)
    }

    pub fn paused(&self) -> bool {
        self.paused_left.is_some()
    }

    pub fn today(&self, day: i32) -> u32 {
        if day == self.day { self.today } else { 0 }
    }

    pub fn pause(&mut self, now_ms: u64) {
        if self.paused_left.is_none() {
            self.paused_left = Some(self.end_ms.saturating_sub(now_ms));
        }
    }

    pub fn resume(&mut self, now_ms: u64) {
        if let Some(left) = self.paused_left.take() {
            self.end_ms = now_ms + left;
        }
    }

    /// «Пропустить сегмент»: it ends now (a skipped work segment does not count).
    pub fn skip(&mut self, now_ms: u64, day: i32) -> FocusEvent {
        self.paused_left = None;
        self.advance(now_ms, day, false)
    }

    fn advance(&mut self, now_ms: u64, day: i32, counts: bool) -> FocusEvent {
        if day != self.day {
            self.day = day;
            self.today = 0;
        }
        match self.segment {
            Segment::Work => {
                if counts {
                    self.today += 1;
                }
                self.streak += 1;
                if self.streak >= self.cfg.cycles_before_long {
                    self.streak = 0;
                    self.segment = Segment::LongBreak;
                    self.end_ms = now_ms + self.cfg.long_break_ms;
                } else {
                    self.segment = Segment::Break;
                    self.end_ms = now_ms + self.cfg.break_ms;
                }
                FocusEvent::WorkEnded
            }
            Segment::Break | Segment::LongBreak => {
                self.segment = Segment::Work;
                self.end_ms = now_ms + self.cfg.work_ms;
                FocusEvent::BreakEnded
            }
        }
    }

    pub fn tick(&mut self, now_ms: u64, day: i32) -> Option<FocusEvent> {
        if self.paused_left.is_some() || now_ms < self.end_ms {
            return None;
        }
        // The new segment starts from the planned boundary (a late tick does not stretch it).
        let at = self.end_ms;
        let ev = self.advance(at, day, true);
        Some(ev)
    }
}

pub use sirtone_core::wave::{Fit, fit};

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    #[test]
    fn twenty_five_five_and_a_long_break_after_four() {
        let cfg = FocusCfg::minutes(25, 5, 15, 4);
        let mut f = FocusCycle::start(cfg, 0, 100, 0);
        let mut now = 0;
        let mut seen = Vec::new();
        for _ in 0..8 {
            now = f.end_ms().unwrap_or(now);
            let ev = f.tick(now, 100);
            seen.push((ev, f.segment));
        }
        use FocusEvent::*;
        use Segment::*;
        assert_eq!(
            seen,
            [
                (Some(WorkEnded), Break),
                (Some(BreakEnded), Work),
                (Some(WorkEnded), Break),
                (Some(BreakEnded), Work),
                (Some(WorkEnded), Break),
                (Some(BreakEnded), Work),
                (Some(WorkEnded), LongBreak),
                (Some(BreakEnded), Work),
            ]
        );
        assert_eq!(f.today(100), 4);
        assert_eq!(f.remaining_ms(now), 25 * MIN);
    }

    #[test]
    fn pause_freezes_and_skip_does_not_count() {
        let cfg = FocusCfg::minutes(25, 5, 15, 4);
        let mut f = FocusCycle::start(cfg, 0, 7, 2);
        f.pause(10 * MIN);
        assert_eq!(f.tick(40 * MIN, 7), None);
        assert_eq!(f.remaining_ms(40 * MIN), 15 * MIN);
        f.resume(40 * MIN);
        assert_eq!(f.remaining_ms(41 * MIN), 14 * MIN);
        assert_eq!(f.skip(41 * MIN, 7), FocusEvent::WorkEnded);
        assert_eq!(f.today(7), 2);
        assert_eq!(f.segment, Segment::Break);
    }

    #[test]
    fn a_new_day_starts_the_count_over() {
        let cfg = FocusCfg::minutes(25, 5, 15, 4);
        let mut f = FocusCycle::start(cfg, 0, 7, 5);
        f.tick(25 * MIN, 8);
        assert_eq!(f.today(8), 1);
        assert_eq!(f.today(9), 0);
    }

    #[test]
    fn packing_fits() {
        // 10 min left: a 4-min track leaves room; a 9-min one ends at the boundary.
        assert_eq!(fit(600_000, 240_000, 90_000), Fit::More);
        assert_eq!(fit(600_000, 590_000, 90_000), Fit::Boundary);
        assert_eq!(fit(600_000, 680_000, 90_000), Fit::Boundary);
        assert_eq!(fit(600_000, 700_000, 90_000), Fit::No);
        // 2 min after it: neither another track nor the boundary.
        assert_eq!(fit(600_000, 480_000, 90_000), Fit::No);
    }
}
