import { hash32 } from "./rng";

/**
 * Textures inside the slits (D-122): raster, grid, bars, glitch blocks. All four have the **same
 * ink coverage** (`COVERAGE`), so when the texture flips every 2–3 frames the mean brightness of
 * the slit does not move — the flicker of style 1 stays between frames of near-equal luminance
 * (WCAG 2.3.1, `01a` §B.5). Tiles are unit squares: `{r: rect}` and `{c: dot}`.
 */
export type TextureKind = "bars" | "grid" | "dots" | "blocks";
export const TEXTURE_KINDS: readonly TextureKind[] = ["bars", "grid", "dots", "blocks"];
export const COVERAGE = 0.4;

export type Prim =
  | { r: [x: number, y: number, w: number, h: number] }
  | { c: [x: number, y: number, radius: number] };

const wrap1 = (v: number) => v - Math.floor(v);

/** A bar of width `w` at `x0` (wraps the tile edge as two rects). */
function band(x0: number, w: number, vertical: boolean): Prim[] {
  const x = wrap1(x0);
  const rect = (a: number, b: number): Prim => ({ r: vertical ? [a, 0, b, 1] : [0, a, 1, b] });
  return x + w <= 1 ? [rect(x, w)] : [rect(x, 1 - x), rect(0, x + w - 1)];
}

export function tileOf(kind: TextureKind, seed: number, phase: number): Prim[] {
  switch (kind) {
    case "bars": {
      const off = phase * 0.2;
      return [0, 1, 2, 3, 4].flatMap((i) => band(i * 0.2 + off, 0.08, true));
    }
    case "grid": {
      // One horizontal and one vertical line: 1 − (1 − t)² = COVERAGE.
      const t = 1 - Math.sqrt(1 - COVERAGE);
      const off = phase * 0.37;
      return [...band(off, t, true), ...band(off * 1.7 + 0.3, t, false)];
    }
    case "dots":
      return [{ c: [0.5, 0.5, Math.sqrt(COVERAGE / Math.PI)] }];
    case "blocks": {
      // Four of ten cells (5 × 2), always — the pattern changes, the coverage does not.
      const cells = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
      for (let i = cells.length - 1; i > 0; i--) {
        const j = hash32(seed, phase, i) % (i + 1);
        [cells[i], cells[j]] = [cells[j] as number, cells[i] as number];
      }
      return cells
        .slice(0, 4)
        .map((c) => ({ r: [(c % 5) * 0.2, Math.floor(c / 5) * 0.5, 0.2, 0.5] }));
    }
  }
}

/** The share of the tile that is ink (sampled, so overlaps count once): for tests and tuning. */
export function inkCoverage(prims: Prim[], n = 80): number {
  let on = 0;
  for (let iy = 0; iy < n; iy++) {
    for (let ix = 0; ix < n; ix++) {
      const x = (ix + 0.5) / n;
      const y = (iy + 0.5) / n;
      const hit = prims.some((p) =>
        "r" in p
          ? x >= p.r[0] && x < p.r[0] + p.r[2] && y >= p.r[1] && y < p.r[1] + p.r[3]
          : (x - p.c[0]) ** 2 + (y - p.c[1]) ** 2 <= p.c[2] ** 2,
      );
      if (hit) on++;
    }
  }
  return on / (n * n);
}
