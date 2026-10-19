import type { VizColors, VizSwatch } from "../ipc/bindings";
import { hexToOklch, hexToRgb, type Oklch, oklchToHex, rgbToHex } from "../lib/color";
import cfg from "../../../../../assets/viz/waves.json";

/**
 * The «Волны» palette (T4.2, FR-VIZ-003/006, `06` §7.2): the track's colour from its vibe in
 * OKLCH (hue by the vibe corner, chroma ∝ arousal, lightness by valence), mixed in OKLab with
 * «Цвет вашей музыки» (w = 0.35) and — with «Подстраиваться под акцент» — the theme accent
 * (w = 0.25). Copy of Яндекс «Моя волна» (colour by mood + the user's colour, `01a` §B.3).
 * All numbers live in `assets/viz/waves.json`.
 */
type Range = [number, number];
type Quality = { fps: number; scale: number; minimal: boolean };
export type WavesConfig = {
  hue: { sad: number; angry: number; joy: number; calm: number };
  chroma: Range;
  lightness: Range;
  mix: { collection: number; accent: number; cover: number };
  speed: Range;
  warp: { base: number; bass: number; beat: number; beat_decay: number };
  sparkle: { treble: number; max: number; density: number };
  pulse: { like_ms: number; click_ms: number; lift: number; min_gap_ms: number };
  pointer: { radius: number; decay_ms: number; strength: number; max: number; move_gap_ms: number };
  smoothing: { attack: number; release: number };
  transition_ms: number;
  grain: number;
  jitter_frames: number;
  quality: Record<"1" | "2" | "3" | "4", Quality>;
  fallback: { blur_px: number; drift_s: number };
};
export const WAVES = cfg as unknown as WavesConfig;

/** Four shader colours (sRGB 0…1): main, light, second, deep. Minimal — main and deep only. */
type Rgb3 = [number, number, number];
export type WavesPalette = [Rgb3, Rgb3, Rgb3, Rgb3];

const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const clamp01 = (x: number) => Math.min(1, Math.max(0, x));

type Lab = [number, number, number];
const toLab = (c: Oklch): Lab => {
  const h = (c.h * Math.PI) / 180;
  return [c.l, c.c * Math.cos(h), c.c * Math.sin(h)];
};
const fromLab = ([l, a, b]: Lab): Oklch => {
  const h = (Math.atan2(b, a) * 180) / Math.PI;
  return { l, c: Math.hypot(a, b), h: h < 0 ? h + 360 : h };
};

/** `mix_oklab(a, b, w)`: `w` of `b` into `a`. */
export function mixOklab(a: Oklch, b: Oklch, w: number): Oklch {
  const x = toLab(a);
  const y = toLab(b);
  return fromLab([lerp(x[0], y[0], w), lerp(x[1], y[1], w), lerp(x[2], y[2], w)]);
}

/** The vibe's colour: the corners' hues blended bilinearly on the hue circle. */
export function vibeColor(valence: number, arousal: number, c: WavesConfig = WAVES): Oklch {
  const v = clamp01(valence);
  const a = clamp01(arousal);
  const corners: [number, number][] = [
    [c.hue.sad, (1 - v) * (1 - a)],
    [c.hue.angry, (1 - v) * a],
    [c.hue.joy, v * a],
    [c.hue.calm, v * (1 - a)],
  ];
  let x = 0;
  let y = 0;
  for (const [h, w] of corners) {
    x += w * Math.cos((h * Math.PI) / 180);
    y += w * Math.sin((h * Math.PI) / 180);
  }
  const h = (Math.atan2(y, x) * 180) / Math.PI;
  return {
    l: lerp(c.lightness[0], c.lightness[1], v),
    c: lerp(c.chroma[0], c.chroma[1], a),
    h: h < 0 ? h + 360 : h,
  };
}

const fromSwatch = (s: VizSwatch): Oklch => ({ l: s.l ?? 0.5, c: s.c ?? 0, h: s.h ?? 0 });
/** A cover colour brought into the palette's lightness and chroma ranges. */
const tame = (o: Oklch, c: WavesConfig): Oklch => ({
  l: Math.min(c.lightness[1], Math.max(c.lightness[0], o.l)),
  c: Math.min(c.chroma[1], o.c),
  h: o.h,
});

export type PaletteInput = {
  colors: VizColors | null;
  /** The theme's `accent-fill` (hex), read once from the CSS variable. */
  accent: string;
  followAccent: boolean;
  minimal: boolean;
};

/** The track colour, then the shader's colours. Without a vibe — the cover, else the accent. */
export function buildPalette(p: PaletteInput, c: WavesConfig = WAVES): WavesPalette {
  const col = p.colors;
  const accent = hexToOklch(p.accent);
  let track: Oklch;
  if (col && col.valence !== null && col.arousal !== null)
    track = vibeColor(col.valence, col.arousal, c);
  else if (col?.cover[0]) track = tame(fromSwatch(col.cover[0]), c);
  else track = tame(accent, c);
  let main = col?.collection[0]
    ? mixOklab(track, tame(fromSwatch(col.collection[0]), c), c.mix.collection)
    : track;
  if (p.followAccent) main = mixOklab(main, tame(accent, c), c.mix.accent);
  const second = col?.cover[0]
    ? mixOklab(main, tame(fromSwatch(col.cover[0]), c), c.mix.cover)
    : { ...main, h: (main.h + 325) % 360 };
  const light: Oklch = { l: Math.min(0.85, main.l + 0.12), c: main.c, h: (main.h + 25) % 360 };
  const deep: Oklch = { l: main.l * 0.42, c: main.c * 0.8, h: (main.h + 10) % 360 };
  const rgb = (o: Oklch): [number, number, number] => {
    const { r, g, b } = hexToRgb(oklchToHex(o));
    return [r, g, b];
  };
  return p.minimal
    ? [rgb(main), rgb(main), rgb(deep), rgb(deep)]
    : [rgb(main), rgb(light), rgb(second), rgb(deep)];
}

/** Hex colours for the level-0 gradient (`DESIGN.md` §9): the cover's 2–3, else the palette. */
export function fallbackColors(p: PaletteInput, c: WavesConfig = WAVES): string[] {
  const cover = p.colors?.cover ?? [];
  if (cover.length >= 2) return cover.slice(0, 3).map((s) => oklchToHex(tame(fromSwatch(s), c)));
  const pal = buildPalette({ ...p, minimal: false }, c);
  const hex = ([r, g, b]: [number, number, number]) => rgbToHex({ r, g, b });
  return [hex(pal[0]), hex(pal[2]), hex(pal[3])];
}
