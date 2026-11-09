/**
 * Pure colour math (DESIGN.md §4, docs/06 §4.2, D-58): sRGB ↔ OKLab/OKLCH, WCAG contrast, and the
 * generator that turns one colour («Свой», a preset seed, a seasonal seed) into the three user
 * accent tokens. No token data here — `color.ts` binds it to the generated tokens, and
 * `scripts/gen-tokens.mjs` imports this file directly (T9.1/T9.3).
 */

export type Rgb = { r: number; g: number; b: number }; // 0..1, gamma-encoded sRGB
export type Oklch = { l: number; c: number; h: number }; // h in degrees
export type Theme = "dark" | "light";

/** Since 2.1 (D-155) the user's colour changes only these three; `primary*` are neutral tokens. */
export interface AccentSet {
  accentFill: string;
  accentText: string;
  accentOn: string;
}

/** What an accent must work on: the surfaces of one theme and the two inks for content on a fill. */
export interface AccentContext {
  backgrounds: Record<Theme, string[]>;
  /** Dark and light content colours for `accentOn`. */
  inkDark: string;
  inkLight: string;
}

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

const minContrast = (hex: string, bgs: string[]) => Math.min(...bgs.map((bg) => contrast(hex, bg)));

/** Best of the two inks on a fill. */
export function inkOnWith(fill: string, ctx: AccentContext): string {
  return contrast(fill, ctx.inkDark) >= contrast(fill, ctx.inkLight) ? ctx.inkDark : ctx.inkLight;
}

/**
 * One colour → 3 tokens with the DESIGN.md §4 guarantees: fill ≥ 3:1 to every surface and
 * content on it ≥ 4.5:1; text (thin lines) ≥ 4.5:1 to every surface.
 */
export function generateAccentSetWith(
  seedHex: string,
  theme: Theme,
  ctx: AccentContext,
): AccentSet {
  const bgs = ctx.backgrounds[theme];
  const seed = hexToOklch(seedHex);
  const accentFill = adjustL(
    seed,
    theme,
    (h) => minContrast(h, bgs) >= 3 && contrast(h, inkOnWith(h, ctx)) >= 4.5,
  );
  const accentText = adjustL(hexToOklch(accentFill), theme, (h) => minContrast(h, bgs) >= 4.5);
  return { accentFill, accentText, accentOn: inkOnWith(accentFill, ctx) };
}
