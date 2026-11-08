//! «Звук» on the phone (D-148, `mobile/full-port.md` §1): the PC's equalizer presets, loudness
//! normalization and transitions, decided by the same pure code as on the PC — the phone only
//! executes. Presets from `assets/eq/presets.json`, transition numbers from
//! `assets/automix/automix.json`; nothing hand-typed here.

use std::sync::OnceLock;

use sirtone_core::loudness::{Loudness, gain_db};
use sirtone_playback::automix::{self, Intensity, MixCfg, MixCtx, MixParams, MixTrack, Mode};

use crate::library::Library;

/// One equalizer preset: its id (the app names it) and ten bands, dB.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct EqPreset {
    pub id: String,
    pub bands_db: Vec<f32>,
}

/// The ten bands' centres, Hz (for the labels).
#[uniffi::export]
pub fn eq_bands_hz() -> Vec<f32> {
    presets_doc()
        .get("bands_hz")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

#[uniffi::export]
pub fn eq_presets() -> Vec<EqPreset> {
    presets_doc()
        .get("presets")
        .and_then(serde_json::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|p| {
                    Some(EqPreset {
                        id: p.get("id")?.as_str()?.to_owned(),
                        bands_db: serde_json::from_value(p.get("bands_db")?.clone()).ok()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn presets_doc() -> &'static serde_json::Value {
    static D: OnceLock<serde_json::Value> = OnceLock::new();
    D.get_or_init(|| {
        serde_json::from_str(include_str!("../../../assets/eq/presets.json")).unwrap_or_default()
    })
}

fn mix_params() -> Option<&'static MixParams> {
    static P: OnceLock<Option<MixParams>> = OnceLock::new();
    P.get_or_init(|| {
        MixParams::from_json(include_str!("../../../assets/automix/automix.json")).ok()
    })
    .as_ref()
}

/// How A hands over to B (positions in each track's own time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct MixPlan {
    /// In A: B starts sounding here …
    pub a_out_ms: u64,
    /// … and A is silent from here.
    pub a_stop_ms: u64,
    /// Where B starts (its lead silence skipped).
    pub b_start_ms: u64,
    /// Equal-power curves over the overlap; `false` — A's own fade is kept, B comes in whole.
    pub equal_power: bool,
}

fn mix_track(i: sirtone_library::mixinfo::MixInfo) -> MixTrack {
    MixTrack {
        duration_ms: i.duration_ms,
        lead_silence_ms: i.lead_silence_ms,
        tail_silence_ms: i.tail_silence_ms,
        lufs: i.lufs,
        env_head: i.env_head,
        env_tail: i.env_tail,
        genre: i.genre,
        bpm: i.bpm,
        camelot: i.camelot,
        energy: i.energy,
        head_grid: None,
        tail_grid: None,
    }
}

#[uniffi::export]
impl Library {
    /// The normalization gain for a track, dB (`target` −19 / −14 / −11 LUFS): the analysis'
    /// measurement first, the ReplayGain/R128 tags next; unknown → 0 (never a blind boost).
    pub fn norm_gain_db(&self, track_id: i64, target_lufs: f32) -> f32 {
        let l = self.with(|c| {
            c.query_row(
                "SELECT coalesce(lufs_i, tag_lufs), CASE WHEN lufs_i IS NOT NULL
                        THEN true_peak_db ELSE tag_peak_db END FROM track WHERE id = ?1",
                [track_id],
                |r| {
                    Ok(Loudness {
                        lufs: r.get::<_, Option<f64>>(0)?.map(|v| v as f32),
                        peak_db: r.get::<_, Option<f64>>(1)?.map(|v| v as f32),
                    })
                },
            )
            .unwrap_or_default()
        });
        gain_db(target_lufs, l)
    }

    /// The transition from A to B: `mode` `soft` / `smart` (automix) or `crossfade:<s>`;
    /// `None` — play B after A as is (gapless). Beatmatching needs a time stretch the phone
    /// does not do: such a plan is made again as «Мягко».
    pub fn mix_plan(
        &self,
        a_id: i64,
        b_id: i64,
        mode: String,
        album_in_order: bool,
        sleep: bool,
    ) -> Option<MixPlan> {
        let p = mix_params()?;
        let (a, b) = self.with(|c| {
            (
                sirtone_library::mixinfo::of_track(c, a_id).ok().flatten(),
                sirtone_library::mixinfo::of_track(c, b_id).ok().flatten(),
            )
        });
        let (a, b) = (mix_track(a?), mix_track(b?));
        let mode = match mode.as_str() {
            "soft" => Mode::Automix(Intensity::Soft),
            "smart" => Mode::Automix(Intensity::Smart),
            m => Mode::Crossfade(
                m.strip_prefix("crossfade:")?
                    .parse::<f32>()
                    .ok()?
                    .clamp(1.0, 12.0),
            ),
        };
        let ctx = MixCtx {
            album_in_order,
            sleep,
        };
        let cfg = |mode| MixCfg {
            mode,
            skip_albums: true,
            genre_blacklist: Vec::new(),
        };
        let mut plan = automix::plan(&a, &b, ctx, &cfg(mode), p);
        if plan.level == automix::Level::L3 || (plan.rate - 1.0).abs() > 1e-6 {
            plan = automix::plan(&a, &b, ctx, &cfg(Mode::Automix(Intensity::Soft)), p);
        }
        if plan.level == automix::Level::L0 {
            return None;
        }
        Some(MixPlan {
            a_out_ms: plan.a_out_ms,
            a_stop_ms: plan.a_stop_ms.max(plan.a_out_ms),
            b_start_ms: plan.b_start_ms,
            equal_power: plan.a_curve == automix::Curve::EqualPower
                || plan.b_curve == automix::Curve::EqualPower,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_come_from_the_pc_file() {
        let p = eq_presets();
        assert!(p.len() >= 10);
        assert!(p.iter().all(|x| x.bands_db.len() == 10));
        assert!(p.iter().any(|x| x.id == "flat"));
        assert_eq!(eq_bands_hz().len(), 10);
    }

    #[test]
    fn unknown_tracks_are_neither_boosted_nor_mixed() {
        let dir = std::env::temp_dir().join(format!("sirtone-sound-{}", std::process::id()));
        let lib = Library::open(dir.to_string_lossy().into_owned()).unwrap();
        assert_eq!(lib.norm_gain_db(1, -14.0), 0.0);
        assert_eq!(lib.mix_plan(1, 2, "soft".into(), false, false), None);
        drop(lib);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
