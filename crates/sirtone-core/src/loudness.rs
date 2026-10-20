//! Loudness normalization (T4.11, FR-PLAY-008, `04` §5 — copy of F-02: Spotify's three targets,
//! album gain, ReplayGain 2.0 / R128 tags, EBU R128 measurement). Pure: the player gives the
//! numbers it knows, this decides the gain.
//!
//! Never boost what cannot be bounded: unknown loudness → 0 dB; known loudness with unknown peak
//! → cuts only; the peak cap `−1 dBTP − peak` holds — except in «Громко» with the limiter
//! (T4.13) behind it, as `04` §5 allows ([`gain_db_limited`]).

/// What is known about a track's (or an album's) loudness.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Loudness {
    /// Integrated loudness, LUFS: measured (B0, EBU R128) or from tags.
    pub lufs: Option<f32>,
    /// Peak, dBFS: true peak when measured, sample peak from ReplayGain tags (a weaker bound —
    /// true peak can be up to ~0.5 dB higher; tags are only used without a measurement).
    pub peak_db: Option<f32>,
}

/// The three targets (Spotify Quiet / Normal / Loud, `01b` F-02).
pub const TARGETS: [f32; 3] = [-19.0, -14.0, -11.0];
/// Boost and cut limits (`04` §5).
pub const MAX_BOOST_DB: f32 = 6.0;
pub const MAX_CUT_DB: f32 = 12.0;
/// The true-peak ceiling (AES TD1008, FR-PLAY-008).
pub const PEAK_CEILING_DB: f32 = -1.0;

/// The gain, dB, that brings `l` to `target`: `clamp(target − L, −12, +6)`, never above
/// `−1 − peak`; unknown loudness → 0; unknown peak → no boost.
pub fn gain_db(target: f32, l: Loudness) -> f32 {
    let Some(lufs) = l.lufs.filter(|v| v.is_finite()) else {
        return 0.0;
    };
    let mut g = (target - lufs).clamp(-MAX_CUT_DB, MAX_BOOST_DB);
    match l.peak_db.filter(|v| v.is_finite()) {
        Some(peak) => g = g.min(PEAK_CEILING_DB - peak),
        None => g = g.min(0.0),
    }
    // Never more cut than the clamp allows (a peak above 0 dBFS must not push it further).
    g.max(-MAX_CUT_DB)
}

/// [`gain_db`] with a limiter behind it (T4.13, `04` §5): «Громко» (−11) may then boost past
/// the peak cap — the limiter keeps the peaks at −1 dBFS; the other targets keep the cap.
pub fn gain_db_limited(target: f32, l: Loudness, limiter: bool) -> f32 {
    if !(limiter && target >= TARGETS[2]) {
        return gain_db(target, l);
    }
    match l.lufs.filter(|v| v.is_finite()) {
        Some(lufs) => (target - lufs).clamp(-MAX_CUT_DB, MAX_BOOST_DB),
        None => 0.0,
    }
}

/// dB → linear amplitude.
pub fn amplitude(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// ReplayGain 2.0 gain (dB, reference −18 LUFS) → LUFS.
pub fn lufs_from_replaygain(gain_db: f32) -> f32 {
    -18.0 - gain_db
}

/// Opus `R128_*_GAIN` (Q7.8, reference −23 LUFS) → LUFS. The Opus output gain is not added:
/// the decoder applies it already.
pub fn lufs_from_r128(q78: i32) -> f32 {
    -23.0 - q78 as f32 / 256.0
}

/// ReplayGain peak (linear sample peak, 1.0 = full scale) → dBFS.
pub fn peak_db_from_linear(p: f32) -> Option<f32> {
    (p > 0.0 && p.is_finite()).then(|| 20.0 * p.log10())
}

/// An album's loudness from its tracks (`(loudness, duration_ms)`): a duration-weighted power
/// average of the integrated loudness and the loudest peak. Only when every track is known —
/// a partial album would drift as analysis goes on, so then `None` (the player uses track
/// gain meanwhile).
pub fn album(tracks: &[(Loudness, u64)]) -> Option<Loudness> {
    if tracks.is_empty() {
        return None;
    }
    let mut energy = 0f64;
    let mut total = 0f64;
    let mut peak: Option<f32> = Some(f32::NEG_INFINITY);
    for (l, ms) in tracks {
        let lufs = l.lufs.filter(|v| v.is_finite())?;
        let w = (*ms).max(1) as f64;
        energy += w * 10f64.powf(f64::from(lufs) / 10.0);
        total += w;
        peak = match (peak, l.peak_db) {
            (Some(a), Some(b)) => Some(a.max(b)),
            _ => None,
        };
    }
    Some(Loudness {
        lufs: Some((10.0 * (energy / total).log10()) as f32),
        peak_db: peak,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(lufs: f32, peak: f32) -> Loudness {
        Loudness {
            lufs: Some(lufs),
            peak_db: Some(peak),
        }
    }

    #[test]
    fn gain_table() {
        // (target, loudness, peak, expected gain)
        let cases = [
            (-14.0, -8.0, -0.1, -6.0),  // loud master: cut to target
            (-14.0, -20.0, -10.0, 6.0), // quiet: boost up to +6
            (-14.0, -30.0, -20.0, 6.0), // very quiet: boost clamped at +6
            (-14.0, -14.0, -1.0, 0.0),  // already there
            (-19.0, 0.0, 0.0, -12.0),   // absurdly loud: cut clamped at −12
            (-14.0, -18.0, -2.0, 1.0),  // boost limited by the peak: −1 − (−2) = +1
            (-11.0, -16.0, -1.5, 0.5),  // «Громко» keeps the peak cap (no limiter yet)
            (-14.0, -10.0, 1.0, -4.0),  // a peak above 0 dBFS does not turn a cut into more
        ];
        for (target, lufs, peak, want) in cases {
            let g = gain_db(target, l(lufs, peak));
            assert!(
                (g - want).abs() < 0.01,
                "{target} {lufs} {peak}: {g} ≠ {want}"
            );
        }
    }

    #[test]
    fn loud_with_the_limiter_passes_the_peak_cap() {
        // «Громко»: −16 LUFS, peak −1.5 → +5 (the limiter holds the peaks), not +0.5.
        assert!((gain_db_limited(-11.0, l(-16.0, -1.5), true) - 5.0).abs() < 1e-4);
        assert!((gain_db_limited(-11.0, l(-16.0, -1.5), false) - 0.5).abs() < 1e-4);
        // «Обычно» keeps the cap even with the limiter.
        assert!((gain_db_limited(-14.0, l(-18.0, -2.0), true) - 1.0).abs() < 1e-4);
        // Still within +6 and nothing for the unknown.
        assert!((gain_db_limited(-11.0, l(-30.0, -20.0), true) - 6.0).abs() < 1e-4);
        assert_eq!(gain_db_limited(-11.0, Loudness::default(), true), 0.0);
    }

    #[test]
    fn unknown_never_boosts() {
        assert_eq!(gain_db(-14.0, Loudness::default()), 0.0);
        let no_peak = Loudness {
            lufs: Some(-20.0),
            peak_db: None,
        };
        assert_eq!(gain_db(-14.0, no_peak), 0.0);
        let loud_no_peak = Loudness {
            lufs: Some(-8.0),
            peak_db: None,
        };
        assert!((gain_db(-14.0, loud_no_peak) + 6.0).abs() < 1e-4);
        assert_eq!(gain_db(-14.0, l(f32::NAN, 0.0)), 0.0);
    }

    #[test]
    fn tags_convert_to_lufs() {
        assert!((lufs_from_replaygain(-6.5) + 11.5).abs() < 1e-4);
        assert!((lufs_from_replaygain(3.0) + 21.0).abs() < 1e-4);
        assert!((lufs_from_r128(-1280) + 18.0).abs() < 1e-4);
        assert!((lufs_from_r128(256) + 24.0).abs() < 1e-4);
        assert!((peak_db_from_linear(1.0).unwrap()).abs() < 1e-6);
        assert!((peak_db_from_linear(0.5).unwrap() + 6.0206).abs() < 1e-3);
        assert_eq!(peak_db_from_linear(0.0), None);
        assert!((amplitude(-6.0206) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn album_gain_is_one_for_all_and_keeps_dynamics() {
        // A quiet intro and two loud songs: one gain for the album, so the intro stays quieter.
        let tracks = [
            (l(-24.0, -12.0), 60_000),
            (l(-10.0, -0.5), 240_000),
            (l(-11.0, -0.8), 240_000),
        ];
        let a = album(&tracks).unwrap();
        // Power average weighted by time: close to the loud songs, above their mean in dB.
        assert!(a.lufs.unwrap() > -11.0 && a.lufs.unwrap() < -10.0, "{a:?}");
        assert_eq!(a.peak_db, Some(-0.5));
        let g = gain_db(-14.0, a);
        let intro_out = -24.0 + g;
        let song_out = -10.0 + g;
        assert!(
            ((song_out - intro_out) - 14.0).abs() < 1e-4,
            "dynamics kept"
        );
        // Track gain would flatten it: the intro boosted, the songs cut.
        let (gi, gs) = (gain_db(-14.0, tracks[0].0), gain_db(-14.0, tracks[1].0));
        assert!((-10.0 + gs) - (-24.0 + gi) < 14.0);
        // One unknown track — no album value yet.
        let partial = [(l(-12.0, -1.0), 1000), (Loudness::default(), 1000)];
        assert_eq!(album(&partial), None);
        // An unknown peak makes the album peak unknown (no boost).
        let no_peak = [
            (l(-20.0, -6.0), 1000),
            (
                Loudness {
                    lufs: Some(-20.0),
                    peak_db: None,
                },
                1000,
            ),
        ];
        assert_eq!(album(&no_peak).unwrap().peak_db, None);
    }
}
