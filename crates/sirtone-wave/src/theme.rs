//! Seasonal and event themes (feature D, docs/14 §12, FR-THEME-001…005): which seed colour the
//! interface accent comes from today, the visualiser palette, the cover placeholder variant and
//! the linked effect. Palettes live in `assets/wave/themes.json` (temporary values until the
//! owner's design, OWN-14). Between seasons the seed is the two seasons mixed in OKLCH by the
//! weights of the local date (§10.1), so it changes once a day.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::ConfigError;
use crate::season::{card_key, dominant};

const SEASONS: [&str; 4] = ["winter", "spring", "summer", "autumn"];

#[derive(Debug, Clone, Deserialize)]
pub struct ThemeDef {
    pub accent: String,
    pub viz: Vec<String>,
    pub effect: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ThemesConfig {
    schema: String,
    pub seasons: BTreeMap<String, ThemeDef>,
    pub events: BTreeMap<String, ThemeDef>,
}

impl ThemesConfig {
    pub fn from_json(s: &str) -> Result<Self, ConfigError> {
        let c: Self = serde_json::from_str(s)?;
        if c.schema != "sirtone.themes/1" {
            return Err(ConfigError::Schema(c.schema));
        }
        for name in SEASONS {
            if !c.seasons.contains_key(name) {
                return Err(ConfigError::Invalid(format!("theme `{name}` missing")));
            }
        }
        for (id, t) in c.seasons.iter().chain(&c.events) {
            if parse_hex(&t.accent).is_none()
                || !(3..=5).contains(&t.viz.len())
                || t.viz.iter().any(|v| parse_hex(v).is_none())
            {
                return Err(ConfigError::Invalid(format!("theme `{id}`")));
            }
        }
        Ok(c)
    }

    /// Every theme id a manual accent may be kept for (seasons and events).
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.seasons
            .keys()
            .chain(self.events.keys())
            .map(String::as_str)
    }
}

impl Default for ThemesConfig {
    // The shipped file is compiled in and checked by `shipped_themes_parse`.
    #[allow(clippy::expect_used)]
    fn default() -> Self {
        Self::from_json(include_str!("../../../assets/wave/themes.json"))
            .expect("shipped themes.json is valid (test)")
    }
}

/// Where the seed colour came from (`14` §12 `source`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSource {
    Season,
    Event,
}

/// The theme of the moment (`14` §12). `id` — the theme a manual accent is kept for (the
/// dominant season or the event); `card` — the seasonal card key (the same date gives the same
/// name as «Сезонная волна»).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeState {
    pub id: String,
    pub card: String,
    pub source: ThemeSource,
    /// The accent seed `#rrggbb`; the 6 tokens come from the UI generator (contrast there).
    pub seed: String,
    pub viz: Vec<String>,
    pub cover_variant: String,
    pub effect: String,
}

/// The theme for the season weights and an accepted event (`None` until events, T5.14).
/// Priority (FR-THEME-002): event (with a palette) > season. The manual accent and the switch
/// itself are the settings' business, above this.
pub fn resolve(cfg: &ThemesConfig, season_w: &[f32; 4], event: Option<&str>) -> ThemeState {
    let card = card_key(season_w).to_owned();
    if let Some((id, t)) = event.and_then(|e| cfg.events.get_key_value(e)) {
        return ThemeState {
            id: id.clone(),
            card,
            source: ThemeSource::Event,
            seed: t.accent.to_ascii_lowercase(),
            viz: t.viz.iter().map(|v| v.to_ascii_lowercase()).collect(),
            cover_variant: id.clone(),
            effect: t.effect.clone(),
        };
    }
    let main = dominant(season_w);
    let mut order = [0usize, 1, 2, 3];
    order.sort_by(|&a, &b| season_w[b].total_cmp(&season_w[a]));
    let second = if order[0] == main { order[1] } else { order[0] };
    let (wm, ws) = (season_w[main].max(0.0), season_w[second].max(0.0));
    let t = if wm + ws > 0.0 { ws / (wm + ws) } else { 0.0 };
    let def = |i: usize| cfg.seasons.get(SEASONS[i]);
    let (Some(a), Some(b)) = (def(main), def(second)) else {
        // `from_json` guarantees all four; keep a sane value anyway.
        return ThemeState {
            id: SEASONS[main].to_owned(),
            card,
            source: ThemeSource::Season,
            seed: String::from("#888888"),
            viz: Vec::new(),
            cover_variant: SEASONS[main].to_owned(),
            effect: String::new(),
        };
    };
    let seed = mix_hex(&a.accent, &b.accent, t);
    let viz = a
        .viz
        .iter()
        .enumerate()
        .map(|(i, v)| match b.viz.get(i) {
            Some(w) => mix_hex(v, w, t),
            None => v.to_ascii_lowercase(),
        })
        .collect();
    ThemeState {
        id: SEASONS[main].to_owned(),
        card,
        source: ThemeSource::Season,
        seed,
        viz,
        cover_variant: SEASONS[main].to_owned(),
        effect: a.effect.clone(),
    }
}

fn parse_hex(s: &str) -> Option<[f64; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let c = |i: usize| {
        u8::from_str_radix(&h[i..i + 2], 16)
            .ok()
            .map(|v| f64::from(v) / 255.0)
    };
    Some([c(0)?, c(2)?, c(4)?])
}

fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB → OKLCH (Björn Ottosson's OKLab), hue in degrees.
fn oklch(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(to_linear);
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    let ok_l = 0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s;
    let ok_a = 1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s;
    let ok_b = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s;
    let h = ok_b.atan2(ok_a).to_degrees().rem_euclid(360.0);
    [ok_l, ok_a.hypot(ok_b), h]
}

fn from_oklch([ok_l, c, h]: [f64; 3]) -> [f64; 3] {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l = (ok_l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (ok_l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (ok_l - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
    [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s,
    ]
    .map(|v| from_linear(v.clamp(0.0, 1.0)))
}

/// `color-mix(in oklch, a, b t)` with the shorter hue arc (the CSS default).
fn mix_hex(a: &str, b: &str, t: f32) -> String {
    let (Some(x), Some(y)) = (parse_hex(a), parse_hex(b)) else {
        return a.to_ascii_lowercase();
    };
    let t = f64::from(t.clamp(0.0, 1.0));
    if t <= 0.0 {
        return a.to_ascii_lowercase();
    }
    let (p, q) = (oklch(x), oklch(y));
    let mut dh = q[2] - p[2];
    if dh > 180.0 {
        dh -= 360.0;
    } else if dh < -180.0 {
        dh += 360.0;
    }
    let m = [
        p[0] + (q[0] - p[0]) * t,
        p[1] + (q[1] - p[1]) * t,
        (p[2] + dh * t).rem_euclid(360.0),
    ];
    let [r, g, b] = from_oklch(m);
    let byte = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{days_from_civil, season_weights};

    fn at(y: i32, m: u32, d: u32) -> ThemeState {
        let cfg = ThemesConfig::default();
        let w = season_weights(days_from_civil(y, m, d), false, 21);
        resolve(&cfg, &w, None)
    }

    fn hue(hex: &str) -> f64 {
        oklch(parse_hex(hex).unwrap_or_default())[2]
    }

    #[test]
    fn shipped_themes_parse() {
        let c = ThemesConfig::default();
        assert_eq!(c.seasons.len(), 4);
        assert!(c.events.contains_key("new_year") && c.events.contains_key("halloween"));
    }

    #[test]
    fn pure_seasons_take_their_palette() {
        let s = at(2026, 7, 15);
        assert_eq!((s.id.as_str(), s.seed.as_str()), ("summer", "#f2725c"));
        assert_eq!(s.viz, ["#f2725c", "#f6c445", "#2ec4b6"]);
        assert_eq!(
            (s.source, s.effect.as_str()),
            (ThemeSource::Season, "sunlight")
        );
        assert_eq!(at(2026, 1, 15).seed, "#5aa9e6");
    }

    #[test]
    fn oklch_round_trip_is_exact() {
        for h in [
            "#5aa9e6", "#4cb86b", "#f2725c", "#e09a2d", "#a4161a", "#16131a",
        ] {
            let p = oklch(parse_hex(h).unwrap_or_default());
            let [r, g, b] = from_oklch(p);
            let byte = |v: f64| (v * 255.0).round() as u8;
            assert_eq!(format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b)), h);
        }
    }

    #[test]
    fn pre_winter_leans_to_winter_by_the_window_weight() {
        // AT-14: 22.11 — the card «Предзимье», winter weight ≈ 0.29 (window 21 days).
        let s = at(2026, 11, 22);
        assert_eq!((s.id.as_str(), s.card.as_str()), ("autumn", "preWinter"));
        // The shorter hue arc autumn → winter goes up through green.
        let whole = (hue("#5aa9e6") - hue("#e09a2d")).rem_euclid(360.0);
        let moved = (hue(&s.seed) - hue("#e09a2d")).rem_euclid(360.0);
        let share = moved / whole;
        assert!(
            (share - 0.286).abs() < 0.03,
            "hue share {share}, seed {}",
            s.seed
        );
    }

    #[test]
    fn a_whole_year_changes_by_the_day_and_matches_the_card() {
        let cfg = ThemesConfig::default();
        let start = days_from_civil(2026, 1, 1);
        let mut changes = 0;
        let mut last = String::new();
        for d in start..start + 365 {
            let w = season_weights(d, false, 21);
            let s = resolve(&cfg, &w, None);
            assert_eq!(s.card, card_key(&w));
            assert_eq!(s.id, SEASONS[dominant(&w)]);
            if s.seed != last {
                changes += 1;
                last = s.seed;
            }
        }
        // Four transition windows of 42 days change every day; the rest stays put.
        assert!((150..=180).contains(&changes), "{changes} daily changes");
    }

    #[test]
    fn southern_hemisphere_is_half_a_year_off() {
        let cfg = ThemesConfig::default();
        let w = season_weights(days_from_civil(2026, 1, 15), true, 21);
        assert_eq!(resolve(&cfg, &w, None).id, "summer");
    }

    #[test]
    fn an_event_with_a_palette_wins_others_keep_the_season() {
        let cfg = ThemesConfig::default();
        let w = season_weights(days_from_civil(2026, 10, 31), false, 21);
        let h = resolve(&cfg, &w, Some("halloween"));
        assert_eq!((h.id.as_str(), h.source), ("halloween", ThemeSource::Event));
        assert_eq!(h.seed, "#f07c1b");
        let other = resolve(&cfg, &w, Some("cosmonautics_day"));
        assert_eq!(
            (other.id.as_str(), other.source),
            ("autumn", ThemeSource::Season)
        );
    }
}
