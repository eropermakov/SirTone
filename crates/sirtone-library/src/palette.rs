//! Cover palettes and the collection colour (T4.2, FR-VIZ-003, `06` §7.2): the dominant colours
//! of a cover by `DESIGN.md` §4 «Авто-режим» — 64 px, OKLab, near-black (L < 0.12) and near-white
//! (L > 0.95) dropped, k-means k = 5, ranked by `share × (0.3 + chroma)` — the top 3 cached in
//! `cover.palette` as OKLCH (Apple Music / AMLL take the «liquid» background from cover colours,
//! `01a` §B.3); and
//! «Цвет вашей музыки» (Яндекс «Моя волна», `01a` §B.3): the same clustering over the palettes of
//! the most-played albums, weighted by plays. Images are decoded outside any database lock.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::covers;
use crate::error::LibraryError;

/// One dominant colour: OKLCH (`h` in degrees) and its share `w` (0…1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
    pub w: f32,
}

/// Clusters (`DESIGN.md` §4) and colours kept (`cover.palette`: «3–5»; the visualiser uses 2–3).
const K: usize = 5;
const KEEP: usize = 3;
/// `DESIGN.md` §4 step 2.
const L_MIN: f32 = 0.12;
const L_MAX: f32 = 0.95;
const ITERATIONS: usize = 10;
/// Albums the collection colour looks at (the most played first).
pub const COLLECTION_ALBUMS: usize = 120;

fn to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB (0…255) → OKLab (Björn Ottosson's matrices, as `shared/lib/color.ts`).
fn oklab(rgb: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = rgb.map(|v| to_linear(f32::from(v) / 255.0));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_99 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn to_swatch(lab: [f32; 3], w: f32) -> Swatch {
    let h = lab[2].atan2(lab[1]).to_degrees();
    Swatch {
        l: lab[0],
        c: lab[1].hypot(lab[2]),
        h: if h < 0.0 { h + 360.0 } else { h },
        w,
    }
}

fn from_swatch(s: &Swatch) -> [f32; 3] {
    let h = s.h.to_radians();
    [s.l, s.c * h.cos(), s.c * h.sin()]
}

fn dist2(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

/// Weighted k-means in OKLab, deterministic: farthest-point seeding from the heaviest point.
/// Returns up to `KEEP` swatches ranked by `share × (0.3 + chroma)`; empty clusters are dropped.
fn kmeans(points: &[([f32; 3], f32)]) -> Vec<Swatch> {
    let total: f32 = points.iter().map(|p| p.1).sum();
    if points.is_empty() || total <= 0.0 {
        return Vec::new();
    }
    let mut centers: Vec<[f32; 3]> = Vec::with_capacity(K);
    if let Some(first) = points.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
        centers.push(first.0);
    }
    while centers.len() < K {
        let far = points
            .iter()
            .map(|p| {
                let d = centers
                    .iter()
                    .map(|c| dist2(&p.0, c))
                    .fold(f32::MAX, f32::min);
                (p.0, d * p.1)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match far {
            Some((c, d)) if d > 1e-6 => centers.push(c),
            _ => break,
        }
    }
    let mut weight = vec![0f32; centers.len()];
    for _ in 0..ITERATIONS {
        let mut sum = vec![[0f32; 3]; centers.len()];
        weight.fill(0.0);
        for (p, w) in points {
            let i = (0..centers.len())
                .min_by(|&a, &b| dist2(p, &centers[a]).total_cmp(&dist2(p, &centers[b])))
                .unwrap_or(0);
            for k in 0..3 {
                sum[i][k] += p[k] * w;
            }
            weight[i] += w;
        }
        for (i, c) in centers.iter_mut().enumerate() {
            if weight[i] > 0.0 {
                *c = sum[i].map(|v| v / weight[i]);
            }
        }
    }
    let mut out: Vec<Swatch> = centers
        .iter()
        .zip(&weight)
        .filter(|(_, w)| **w > 0.0)
        .map(|(c, w)| to_swatch(*c, w / total))
        .collect();
    out.sort_by(|a, b| (b.w * (0.3 + b.c)).total_cmp(&(a.w * (0.3 + a.c))));
    out.truncate(KEEP);
    out
}

/// The palette of one cover from its 64 px variant (decoded here: call it without a lock).
pub fn of_cover(root: &Path, hash: &str) -> Result<Vec<Swatch>, LibraryError> {
    let path = covers::variant(root, hash, 64)?;
    let img = image::ImageReader::open(&path)?
        .with_guessed_format()?
        .decode()
        .map_err(|e| LibraryError::Invalid(e.to_string()))?
        .to_rgb8();
    let all: Vec<([f32; 3], f32)> = img.pixels().map(|p| (oklab(p.0), 1.0)).collect();
    let kept: Vec<_> = all
        .iter()
        .copied()
        .filter(|(lab, _)| (L_MIN..=L_MAX).contains(&lab[0]))
        .collect();
    // A black-and-white cover still has a palette.
    Ok(kmeans(if kept.is_empty() { &all } else { &kept }))
}

/// The cached palette of a cover (`None` — not computed yet).
pub fn cached(conn: &Connection, hash: &str) -> Result<Option<Vec<Swatch>>, LibraryError> {
    let json: Option<Option<String>> = conn
        .query_row("SELECT palette FROM cover WHERE hash = ?1", [hash], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(json.flatten().and_then(|j| serde_json::from_str(&j).ok()))
}

pub fn store(conn: &Connection, hash: &str, palette: &[Swatch]) -> Result<(), LibraryError> {
    let json = serde_json::to_string(palette).map_err(|e| LibraryError::Invalid(e.to_string()))?;
    conn.execute(
        "UPDATE cover SET palette = ?2 WHERE hash = ?1",
        params![hash, json],
    )?;
    Ok(())
}

/// Cover hashes of the most-played albums with their weight (plays + 1: an unplayed library
/// still has a colour).
pub fn collection_covers(conn: &Connection) -> Result<Vec<(String, f32)>, LibraryError> {
    let mut st = conn.prepare_cached(
        "SELECT c.hash, sum(t.play_count) + 1 AS w FROM track t
           JOIN album a ON a.id = t.album_id JOIN cover c ON c.id = a.cover_id
          WHERE t.state = 'ok'
          GROUP BY c.id ORDER BY w DESC, c.id LIMIT ?1",
    )?;
    let rows = st
        .query_map([COLLECTION_ALBUMS as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)? as f32))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// «Цвет вашей музыки»: weighted clusters over the albums' palettes.
pub fn collection(palettes: &[(Vec<Swatch>, f32)]) -> Vec<Swatch> {
    let points: Vec<([f32; 3], f32)> = palettes
        .iter()
        .flat_map(|(p, w)| p.iter().map(move |s| (from_swatch(s), s.w * w)))
        .collect();
    kmeans(&points)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two thirds red, a third blue: two clusters, red first, hues where OKLCH puts them.
    #[test]
    fn a_cover_splits_into_its_dominant_colours() {
        let mut points = Vec::new();
        for i in 0..300 {
            let rgb = if i % 3 == 0 {
                [20, 40, 220]
            } else {
                [220, 30, 30]
            };
            points.push((oklab(rgb), 1.0));
        }
        let p = kmeans(&points);
        assert_eq!(p.len(), 2, "{p:?}");
        assert!((p[0].w - 2.0 / 3.0).abs() < 0.01);
        assert!(p[0].h < 40.0 || p[0].h > 350.0, "red {}", p[0].h);
        assert!((250.0..280.0).contains(&p[1].h), "blue {}", p[1].h);
        let back = from_swatch(&p[0]);
        assert!(dist2(&back, &oklab([220, 30, 30])) < 1e-6);
    }

    /// The collection leans to the albums that are played.
    #[test]
    fn the_collection_follows_the_plays() {
        let red = vec![to_swatch(oklab([220, 30, 30]), 1.0)];
        let teal = vec![to_swatch(oklab([20, 180, 160]), 1.0)];
        let c = collection(&[(red, 10.0), (teal, 1.0)]);
        assert!(c[0].h < 40.0 || c[0].h > 350.0, "{c:?}");
        assert!(c[0].w > 0.85);
        assert!(collection(&[]).is_empty());
    }
}
