import { describe, expect, it } from "vitest";
import THEMES from "../../../../../assets/wave/themes.json";
import {
  BACKGROUNDS,
  contrast,
  generateAccentSet,
  hexToOklch,
  INK_LIGHT,
  oklchToHex,
  type Theme,
} from "../lib/color";
import { buildPalette } from "../viz/palette";

/** `color-mix(in oklch)` with the shorter hue arc — what Rust does between seasons. */
function mid(a: string, b: string, t = 0.5): string {
  const p = hexToOklch(a);
  const q = hexToOklch(b);
  let dh = q.h - p.h;
  if (dh > 180) dh -= 360;
  if (dh < -180) dh += 360;
  return oklchToHex({
    l: p.l + (q.l - p.l) * t,
    c: p.c + (q.c - p.c) * t,
    h: (((p.h + dh * t) % 360) + 360) % 360,
  });
}

const S = THEMES.seasons;
const seeds: [string, string][] = [
  ...Object.entries(S).map(([k, v]) => [k, v.accent] as [string, string]),
  ...Object.entries(THEMES.events).map(([k, v]) => [k, v.accent] as [string, string]),
  ["winter→spring", mid(S.winter.accent, S.spring.accent)],
  ["spring→summer", mid(S.spring.accent, S.summer.accent)],
  ["summer→autumn", mid(S.summer.accent, S.autumn.accent)],
  ["autumn→winter", mid(S.autumn.accent, S.winter.accent)],
  ["preWinter 22.11", mid(S.autumn.accent, S.winter.accent, 0.286)],
];

describe("seasonal themes keep the contrast (T4.18, FR-THEME-004)", () => {
  for (const theme of ["dark", "light"] as Theme[]) {
    for (const [name, seed] of seeds) {
      it(`${name} ${seed} on ${theme}`, () => {
        const s = generateAccentSet(seed, theme);
        const bgs = Object.values(BACKGROUNDS[theme]);
        for (const bg of bgs) {
          expect(contrast(s.accentFill, bg)).toBeGreaterThanOrEqual(3);
          expect(contrast(s.accentText, bg)).toBeGreaterThanOrEqual(4.5);
          expect(contrast(s.primaryText, bg)).toBeGreaterThanOrEqual(4.5);
        }
        expect(contrast(s.accentOn, s.accentFill)).toBeGreaterThanOrEqual(4.5);
        expect(contrast(INK_LIGHT, s.primary)).toBeGreaterThanOrEqual(7);
        // Only the lightness is fitted: the hue of the seed survives (±12° after gamut clipping).
        const dh = Math.abs(hexToOklch(s.accentText).h - hexToOklch(seed).h) % 360;
        expect(Math.min(dh, 360 - dh)).toBeLessThan(12);
      });
    }
  }

  it("the visualiser takes the theme's palette instead of the accent (FR-VIZ-006)", () => {
    const base = { colors: null, accent: "#ccff00", followAccent: true, minimal: false };
    const plain = buildPalette(base);
    const autumn = buildPalette({ ...base, themeViz: S.autumn.viz });
    expect(autumn).not.toEqual(plain);
    // The main colour sits near the theme's first colour's hue.
    const [r, g, b] = autumn[0];
    const h = hexToOklch(oklchToHex(hexToOklch(rgbHex(r, g, b)))).h;
    const want = hexToOklch(S.autumn.viz[0] ?? "#000000").h;
    const dh = Math.abs(h - want) % 360;
    expect(Math.min(dh, 360 - dh)).toBeLessThan(25);
  });
});

function rgbHex(r: number, g: number, b: number): string {
  const x = (v: number) =>
    Math.round(Math.max(0, Math.min(1, v)) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${x(r)}${x(g)}${x(b)}`;
}
