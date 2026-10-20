//! UI sounds (T4.15, FR-PLAY-016/UI-005, `06` §8 — F-32): short sounds synthesised once per
//! device format (`assets/sfx.json`, temporary until the owner's own — OWN-06), triggered through
//! a lock-free queue and mixed by the render thread on top of the music — also while it is
//! paused — with their own volume. Through the engine, so no WebView audio sessions.
//! The render-thread side ([`SfxVoices`]) only reads preloaded buffers: no allocation, no lock.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SfxSpec {
    pub id: String,
    pub partials: Vec<(f32, f32)>,
    pub ms: u32,
    pub attack_ms: u32,
    pub decay_ms: u32,
    pub gain: f32,
    /// Pitch at the end relative to the start (a glide), 1 — none.
    #[serde(default = "one")]
    pub glide: f32,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, Deserialize)]
struct Doc {
    sounds: Vec<SfxSpec>,
}

/// The sounds, in id order.
pub fn specs() -> Vec<SfxSpec> {
    serde_json::from_str::<Doc>(include_str!("../../../assets/sfx.json"))
        .map(|d| d.sounds)
        .unwrap_or_default()
}

/// The id of a sound by name (`click`, `toggle_on`, …).
pub fn id_of(name: &str) -> Option<u8> {
    specs()
        .iter()
        .position(|s| s.id == name)
        .and_then(|i| u8::try_from(i).ok())
}

/// A mono buffer of the sound at `rate`.
pub fn synth(s: &SfxSpec, rate: u32) -> Vec<f32> {
    let r = rate as f32;
    let n = (r * s.ms as f32 / 1000.0) as usize;
    let attack = (r * s.attack_ms as f32 / 1000.0).max(1.0);
    let decay = (r * s.decay_ms as f32 / 1000.0).max(1.0);
    let total: f32 = s.partials.iter().map(|p| p.1).sum::<f32>().max(1e-6);
    let mut phase = vec![0f32; s.partials.len()];
    (0..n)
        .map(|i| {
            let t = i as f32;
            let env = (t / attack).min(1.0) * (-(t / decay)).exp()
                // A short fade at the very end: no click when the buffer stops.
                * ((n - i) as f32 / (r * 0.003)).min(1.0);
            let pitch = 1.0 + (s.glide - 1.0) * t / n.max(1) as f32;
            let mut v = 0.0;
            for (k, (f, g)) in s.partials.iter().enumerate() {
                phase[k] += std::f32::consts::TAU * f * pitch / r;
                v += phase[k].sin() * g;
            }
            v / total * s.gain * env
        })
        .collect()
}

/// The bank at a device rate.
pub fn bank(rate: u32) -> Vec<Vec<f32>> {
    specs().iter().map(|s| synth(s, rate)).collect()
}

const VOICES: usize = 4;

/// The render thread's mixer: up to 4 sounds at once; a new one replaces the oldest.
pub struct SfxVoices {
    bank: Vec<Vec<f32>>,
    triggers: rtrb::Consumer<u8>,
    voices: [(u8, usize); VOICES],
    active: [bool; VOICES],
    next: usize,
}

impl SfxVoices {
    pub fn new(bank: Vec<Vec<f32>>, triggers: rtrb::Consumer<u8>) -> Self {
        Self {
            bank,
            triggers,
            voices: [(0, 0); VOICES],
            active: [false; VOICES],
            next: 0,
        }
    }

    /// Takes the triggers that arrived (once per device buffer).
    pub fn poll(&mut self) {
        while let Ok(id) = self.triggers.pop() {
            if usize::from(id) < self.bank.len() {
                self.voices[self.next] = (id, 0);
                self.active[self.next] = true;
                self.next = (self.next + 1) % VOICES;
            }
        }
    }

    /// Anything sounding.
    pub fn busy(&self) -> bool {
        self.active.iter().any(|a| *a)
    }

    /// The next mono sample of all sounding voices.
    pub fn next_sample(&mut self) -> f32 {
        let mut v = 0.0;
        for (k, (id, pos)) in self.voices.iter_mut().enumerate() {
            if !self.active[k] {
                continue;
            }
            let buf = &self.bank[usize::from(*id)];
            if *pos < buf.len() {
                v += buf[*pos];
                *pos += 1;
            } else {
                self.active[k] = false;
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn focus_boundary_sounds_exist() {
        assert!(id_of("focus_work_end").is_some());
        assert!(id_of("focus_break_end").is_some());
    }

    use super::*;

    #[test]
    fn the_sounds_are_short_quiet_and_end_softly() {
        let s = specs();
        assert!(s.len() >= 8);
        for spec in &s {
            assert!(spec.ms < 150, "{} {} ms", spec.id, spec.ms);
            let b = synth(spec, 48_000);
            let peak = b.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.01 && peak < 0.5, "{} peak {peak}", spec.id);
            // Ends at (almost) zero: no click.
            assert!(b.last().unwrap().abs() < 0.01, "{}", spec.id);
        }
        assert_eq!(id_of("click"), Some(0));
        assert_eq!(id_of("nope"), None);
    }

    #[test]
    fn voices_mix_and_finish() {
        let (mut p, c) = rtrb::RingBuffer::<u8>::new(8);
        let mut v = SfxVoices::new(vec![vec![0.5; 10], vec![0.25; 5]], c);
        p.push(0).unwrap();
        p.push(1).unwrap();
        v.poll();
        assert!((v.next_sample() - 0.75).abs() < 1e-6);
        for _ in 0..4 {
            v.next_sample();
        }
        assert!((v.next_sample() - 0.5).abs() < 1e-6, "the short one ended");
        for _ in 0..10 {
            v.next_sample();
        }
        assert!(!v.busy());
        // An unknown id is ignored.
        p.push(9).unwrap();
        v.poll();
        assert!(!v.busy());
    }
}
