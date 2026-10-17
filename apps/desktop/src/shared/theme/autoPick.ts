import { ACCENT_SLUGS, type AccentSlug } from "./theme";

/**
 * «Авто» steps 1–5 (DESIGN.md §4) as a pure function — no DOM, unit-tested.
 */
type Lab = [number, number, number];

const lin = (u: number) => (u <= 0.04045 ? u / 12.92 : ((u + 0.055) / 1.055) ** 2.4);

export function rgbToOklab(r: number, g: number, b: number): Lab {
  const lr = lin(r / 255);
  const lg = lin(g / 255);
  const lb = lin(b / 255);
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

const hexToLab = (hex: string): Lab => {
  const h = hex.trim().replace("#", "");
  return rgbToOklab(
    Number.parseInt(h.slice(0, 2), 16),
    Number.parseInt(h.slice(2, 4), 16),
    Number.parseInt(h.slice(4, 6), 16),
  );
};
const dist2 = (a: Lab, b: Lab) => (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2 + (a[2] - b[2]) ** 2;
const chroma = (p: Lab) => Math.hypot(p[1], p[2]);

/** Steps 2–5 on RGBA pixels; `fills` — light-theme fill of each set. */
export function pickSlug(rgba: Uint8ClampedArray, fills: Record<string, string>): AccentSlug {
  const pts: Lab[] = [];
  for (let i = 0; i + 3 < rgba.length; i += 4) {
    if ((rgba[i + 3] ?? 0) < 128) continue;
    const p = rgbToOklab(rgba[i] ?? 0, rgba[i + 1] ?? 0, rgba[i + 2] ?? 0);
    if (p[0] >= 0.12 && p[0] <= 0.95) pts.push(p);
  }
  if (pts.length === 0) return "sirkov";
  const k = Math.min(5, pts.length);
  // Deterministic seeds spread over the image.
  let centers: Lab[] = Array.from(
    { length: k },
    (_, i) => pts[Math.floor((i * pts.length) / k)] as Lab,
  );
  let counts = new Array<number>(k).fill(0);
  for (let iter = 0; iter < 12; iter++) {
    const sums: Lab[] = centers.map(() => [0, 0, 0]);
    counts = new Array<number>(k).fill(0);
    for (const p of pts) {
      let best = 0;
      for (let c = 1; c < k; c++)
        if (dist2(p, centers[c] as Lab) < dist2(p, centers[best] as Lab)) best = c;
      const s = sums[best] as Lab;
      s[0] += p[0];
      s[1] += p[1];
      s[2] += p[2];
      counts[best] = (counts[best] ?? 0) + 1;
    }
    centers = centers.map((c, i) => {
      const n = counts[i] ?? 0;
      const s = sums[i] as Lab;
      return n ? [s[0] / n, s[1] / n, s[2] / n] : c;
    });
  }
  let dom = 0;
  let score = -1;
  for (let c = 0; c < k; c++) {
    const v = ((counts[c] ?? 0) / pts.length) * (0.3 + chroma(centers[c] as Lab));
    if (v > score) {
      score = v;
      dom = c;
    }
  }
  const d = centers[dom] as Lab;
  if (chroma(d) < 0.04) return d[0] < 0.3 || d[0] > 0.85 ? "black" : "gray";
  let bestSlug: AccentSlug = "sirkov";
  let bestD = Number.POSITIVE_INFINITY;
  for (const slug of ACCENT_SLUGS) {
    if (slug === "gray" || slug === "black") continue;
    const fill = fills[slug];
    if (!fill) continue;
    const dd = dist2(d, hexToLab(fill));
    if (dd < bestD) {
      bestD = dd;
      bestSlug = slug;
    }
  }
  return bestSlug;
}
