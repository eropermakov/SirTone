/**
 * Colour math for the 6-token colour model (DESIGN.md §4, docs/06 §4.2, D-58):
 * sRGB ↔ OKLab/OKLCH, WCAG contrast, and the generator that turns one colour
 * («Свой» or a seasonal theme seed) into a full accent set.
 */

export type Rgb = { r: number; g: number; b: number }; // 0..1, gamma-encoded sRGB
export type Oklch = { l: number; c: number; h: number }; // h in degrees
export type Theme = "dark" | "light";

export interface AccentSet {
  accentFill: string;
  accentText: string;
  accentOn: string;
  primary: string;
  primaryText: string;
  onPrimary: string;
}

/** Neutral backgrounds the accent must work on (DESIGN.md §3). */
export const BACKGROUNDS: Record<Theme, { base: string; layer: string; flyout: string }> = {
  dark: { base: "#161111", layer: "#1c1616", flyout: "#2b2323" },
  light: { base: "#f2eee6", layer: "#f9f6f0", flyout: "#fdfcf9" },
};
export const INK_DARK = "#1c1616";
export const INK_LIGHT = "#f9f6f0";

export function hexToRgb(hex: string): Rgb {
  const h = hex.replace("#", "");
  const n = Number.parseInt(
    h.length === 3
      ? h
          .split("")
          .map((x) => x + x)
          .join("")
      : h,
    16,
  );
  return { r: ((n >> 16) & 255) / 255, g: ((n >> 8) & 255) / 255, b: (n & 255) / 255 };
}

export function rgbToHex({ r, g, b }: Rgb): string {
  const to = (v: number) =>
    Math.round(Math.min(1, Math.max(0, v)) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${to(r)}${to(g)}${to(b)}`;
}

const toLinear = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
const toGamma = (v: number) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055);

/** WCAG 2.x relative luminance. */
export function luminance(hex: string): number {
  const { r, g, b } = hexToRgb(hex);
  return 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
}

/** WCAG 2.x contrast ratio (1..21). */
export function contrast(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

export function hexToOklch(hex: string): Oklch {
  const { r, g, b } = hexToRgb(hex);
  const lr = toLinear(r);
  const lg = toLinear(g);
  const lb = toLinear(b);
  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  const h = (Math.atan2(B, A) * 180) / Math.PI;
  return { l: L, c: Math.hypot(A, B), h: h < 0 ? h + 360 : h };
}

function oklchToLinear({ l: L, c, h }: Oklch): [number, number, number] {
  const A = c * Math.cos((h * Math.PI) / 180);
  const B = c * Math.sin((h * Math.PI) / 180);
  const l = (L + 0.3963377774 * A + 0.2158037573 * B) ** 3;
  const m = (L - 0.1055613458 * A - 0.0638541728 * B) ** 3;
  const s = (L - 0.0894841775 * A - 1.291485548 * B) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}

/** OKLCH → hex, reducing chroma until the colour fits the sRGB gamut. */
export function oklchToHex(c: Oklch): string {
  let chroma = c.c;
  for (let i = 0; i < 40; i++) {
    const [r, g, b] = oklchToLinear({ ...c, c: chroma });
    const eps = 1e-4;
    if ([r, g, b].every((v) => v >= -eps && v <= 1 + eps)) {
      return rgbToHex({
        r: toGamma(Math.max(0, r)),
        g: toGamma(Math.max(0, g)),
        b: toGamma(Math.max(0, b)),
      });
    }
    chroma *= 0.9;
  }
  const [r, g, b] = oklchToLinear({ ...c, c: 0 });
  return rgbToHex({ r: toGamma(r), g: toGamma(g), b: toGamma(b) });
}

/** Moves lightness (same hue) until `ok(hex)` holds; direction = toward more contrast with the theme. */
function adjustL(seed: Oklch, theme: Theme, ok: (hex: string) => boolean): string {
  const step = theme === "dark" ? 0.01 : -0.01;
  let cur = { ...seed };
  for (let i = 0; i < 100; i++) {
    const hex = oklchToHex(cur);
    if (ok(hex)) return hex;
    cur = { ...cur, l: Math.min(0.99, Math.max(0.01, cur.l + step)) };
  }
  return oklchToHex(cur);
}

const bgList = (theme: Theme) => Object.values(BACKGROUNDS[theme]);
const minContrast = (hex: string, theme: Theme) =>
  Math.min(...bgList(theme).map((bg) => contrast(hex, bg)));

/** Best of the two inks on a fill. */
export function inkOn(fill: string): string {
  return contrast(fill, INK_DARK) >= contrast(fill, INK_LIGHT) ? INK_DARK : INK_LIGHT;
}

/**
 * One colour → 6 tokens with DESIGN.md §4 guarantees:
 * fill ≥ 3:1 to backgrounds, text & primary-text ≥ 4.5:1, on ≥ 4.5:1 to fill, on-primary ≥ 7:1 to primary.
 */
export function generateAccentSet(seedHex: string, theme: Theme): AccentSet {
  const seed = hexToOklch(seedHex);
  const accentFill = adjustL(
    seed,
    theme,
    (h) => minContrast(h, theme) >= 3 && contrast(h, inkOn(h)) >= 4.5,
  );
  const accentText = adjustL(hexToOklch(accentFill), theme, (h) => minContrast(h, theme) >= 4.5);
  const deep: Oklch = { l: theme === "dark" ? 0.3 : 0.38, c: Math.min(seed.c, 0.12), h: seed.h };
  let primary = oklchToHex(deep);
  for (let i = 0; i < 60 && contrast(primary, INK_LIGHT) < 7; i++) {
    deep.l -= 0.01;
    primary = oklchToHex(deep);
  }
  const ptSeed: Oklch =
    theme === "dark" ? { l: 0.62, c: Math.min(seed.c, 0.12), h: seed.h } : { ...deep };
  const primaryText = adjustL(ptSeed, theme, (h) => minContrast(h, theme) >= 4.5);
  return {
    accentFill,
    accentText,
    accentOn: inkOn(accentFill),
    primary,
    primaryText,
    onPrimary: INK_LIGHT,
  };
}
