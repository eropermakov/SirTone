//! When to duck (T4.14, FR-PLAY-011, `04` §7 — copy of F-04): another application's session
//! peak at or above the threshold continuously for `onset_ms` ducks Sirtone; `hold_ms` after it
//! falls quiet Sirtone comes back. Short sounds (a notification) never duck. Pure: the caller
//! feeds the loudest foreign peak every poll.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Sensitivity {
    pub threshold_db: f32,
    pub onset_ms: u64,
}

/// `assets/duck.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct DuckParams {
    pub poll_ms: u64,
    pub hold_ms: u64,
    pub sensitivity: std::collections::HashMap<String, Sensitivity>,
    /// Executables never counted (capture tools carry Sirtone's own sound).
    #[serde(default)]
    pub builtin_ignore: Vec<String>,
    /// Call apps (T5.18): capturing from the communications microphone = a call.
    #[serde(default)]
    pub call_apps: Vec<String>,
    #[serde(default = "call_poll_default")]
    pub call_poll_ms: u64,
}

fn call_poll_default() -> u64 {
    1000
}

impl DuckParams {
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// «Пауза во время звонка» (T5.18, FR-PLAY-011, D-49): pauses when a call begins while music
/// plays, and plays again when it ends — only what it paused itself (a pause by the person
/// stays a pause).
#[derive(Debug, Default)]
pub struct CallGuard {
    in_call: bool,
    paused_by_us: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallAction {
    None,
    Pause,
    Resume,
}

impl CallGuard {
    pub fn step(&mut self, call: bool, playing: bool, enabled: bool) -> CallAction {
        let began = call && !self.in_call;
        let ended = !call && self.in_call;
        self.in_call = call;
        if began && enabled && playing {
            self.paused_by_us = true;
            return CallAction::Pause;
        }
        if call && playing {
            // The person pressed play during the call: theirs now.
            self.paused_by_us = false;
        }
        if ended && std::mem::take(&mut self.paused_by_us) && !playing {
            return CallAction::Resume;
        }
        CallAction::None
    }

    pub fn in_call(&self) -> bool {
        self.in_call
    }
}

#[derive(Debug, Default)]
pub struct Ducker {
    loud_since: Option<u64>,
    quiet_since: Option<u64>,
    ducked: bool,
}

impl Ducker {
    /// The state after a poll at `now_ms` with the loudest foreign peak `peak_db`.
    pub fn step(&mut self, now_ms: u64, peak_db: f32, s: Sensitivity, hold_ms: u64) -> bool {
        let loud = peak_db >= s.threshold_db;
        if loud {
            self.quiet_since = None;
            let since = *self.loud_since.get_or_insert(now_ms);
            if now_ms - since >= s.onset_ms {
                self.ducked = true;
            }
        } else {
            self.loud_since = None;
            if self.ducked {
                let since = *self.quiet_since.get_or_insert(now_ms);
                if now_ms - since >= hold_ms {
                    self.ducked = false;
                    self.quiet_since = None;
                }
            }
        }
        self.ducked
    }

    pub fn ducked(&self) -> bool {
        self.ducked
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_call_pauses_only_what_it_paused() {
        let mut g = CallGuard::default();
        // Off — nothing.
        assert_eq!(g.step(true, true, false), CallAction::None);
        assert_eq!(g.step(false, true, false), CallAction::None);
        // Playing, a call: pause; the call ends: play again.
        assert_eq!(g.step(true, true, true), CallAction::Pause);
        assert_eq!(g.step(true, false, true), CallAction::None);
        assert_eq!(g.step(false, false, true), CallAction::Resume);
        // Paused by the person before the call: stays paused after it.
        assert_eq!(g.step(true, false, true), CallAction::None);
        assert_eq!(g.step(false, false, true), CallAction::None);
        // Pressed play during the call: no resume needed, nothing paused after.
        assert_eq!(g.step(true, true, true), CallAction::Pause);
        assert_eq!(g.step(true, true, true), CallAction::None);
        assert_eq!(g.step(false, true, true), CallAction::None);
    }

    use super::*;

    fn params() -> DuckParams {
        DuckParams::from_json(include_str!("../../../assets/duck.json")).unwrap()
    }

    /// Feeds a peak (dB) every 100 ms for `ms`, returns the state at each step.
    fn run(
        d: &mut Ducker,
        t: &mut u64,
        ms: u64,
        peak: f32,
        s: Sensitivity,
        hold: u64,
    ) -> Vec<bool> {
        let mut out = Vec::new();
        let end = *t + ms;
        while *t < end {
            out.push(d.step(*t, peak, s, hold));
            *t += 100;
        }
        out
    }

    #[test]
    fn a_video_ducks_after_700_ms_and_comes_back_a_second_after_it_stops() {
        let p = params();
        let s = p.sensitivity["medium"];
        let mut d = Ducker::default();
        let mut t = 0;
        run(&mut d, &mut t, 1_000, -90.0, s, p.hold_ms);
        // A video starts at 1.0 s: not before 1.7 s, then ducked.
        let v = run(&mut d, &mut t, 3_000, -20.0, s, p.hold_ms);
        assert!(!v[6] && v[7], "{v:?}");
        // It stops at 4.0 s: still ducked for the hold, back by 5.0 s.
        let q = run(&mut d, &mut t, 1_500, -90.0, s, p.hold_ms);
        assert!(q[9] && !q[10], "{q:?}");
    }

    #[test]
    fn notifications_and_quiet_sessions_never_duck() {
        let p = params();
        let s = p.sensitivity["medium"];
        let mut d = Ducker::default();
        let mut t = 0;
        // A 400 ms ding, quiet, another ding: never 700 ms in a row.
        for _ in 0..5 {
            assert!(
                !run(&mut d, &mut t, 400, -10.0, s, p.hold_ms)
                    .iter()
                    .any(|x| *x)
            );
            assert!(
                !run(&mut d, &mut t, 300, -90.0, s, p.hold_ms)
                    .iter()
                    .any(|x| *x)
            );
        }
        // An open but silent session (a paused player: active, peak ~ −120 dB).
        assert!(
            !run(&mut d, &mut t, 5_000, -120.0, s, p.hold_ms)
                .iter()
                .any(|x| *x)
        );
        // A brief dip in a long sound does not undo the ducking.
        run(&mut d, &mut t, 2_000, -20.0, s, p.hold_ms);
        let dip = run(&mut d, &mut t, 300, -90.0, s, p.hold_ms);
        assert!(dip.iter().all(|x| *x));
        assert!(d.ducked());
    }
}
