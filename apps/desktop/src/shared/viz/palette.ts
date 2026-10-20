import cfg from "../../../../../assets/viz/waves.json";
import type { VizColors, VizSwatch } from "../ipc/bindings";
import { hexToOklch, hexToRgb, type Oklch, oklchToHex, rgbToHex } from "../lib/color";

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
  warp: { smooth_s: number };
  /** The music speeds the flow up (D-107): bass and the beat's envelope, glided. */
  flow: {
    bass: number;
    beat: number;
    beat_decay: number;
    attack_s: number;
    release_s: number;
    max: number;
  };
  sparkle: { treble: number; max: number; density: number; smooth_s: number };
  pulse: { like_ms: number; click_ms: number; lift: number; min_gap_ms: number };
  pointer: { radius: number; decay_ms: number; strength: number; max: number; move_gap_ms: number };
  motion: {
    speed_w: { arousal: number; energy: number };
    warp_base: Range;
    warp_w: { energy: number; aggression: number };
    beat_gain: Range;
    sparkle_gain: Range;
    grain: Range;
  };
  smoothing: { attack: number; release: number };
  transition_ms: number;
  jitter_frames: number;
  quality: Record<"1" | "2" | "3" | "4", Quality>;
  night: { hide_after_ms: number };
  sleep: { speed: number; speed_final: number; dim: number; black_ms: number };
  focus: { speed: number };
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
  /** The seasonal/event theme's visualiser palette (T4.18, FR-VIZ-006): mixed instead of the
   * accent; absent — the accent (a theme's own manual accent also means the accent). */
  themeViz?: string[] | null;
};

/** The track colour, then the shader's colours. Without a vibe — the cover, else the accent. */
export function buildPalette(p: PaletteInput, c: WavesConfig = WAVES): WavesPalette {
  const col = p.colors;
  const tv = p.themeViz?.length ? p.themeViz.map(hexToOklch) : null;
  const accent = tv?.[0] ?? hexToOklch(p.accent);
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
    : tv?.[1]
      ? mixOklab(main, tame(tv[1], c), c.mix.cover)
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

/** How the field moves for a track (T4.5, FR-VIZ-002): a continuous map of the vibe vector. */
export type Motion = {
  speed: number;
  warpBase: number;
  beatGain: number;
  sparkleGain: number;
  grain: number;
};

/** Palette + motion: what a track looks like; eased as one on a track change. */
export type Look = { palette: WavesPalette; motion: Motion };

type VibeVector = Pick<
  VizColors,
  "arousal" | "energy" | "aggression" | "danceability" | "acousticness"
>;

/** Unknown features (not analysed) sit in the middle. */
export function motionOf(v: Partial<VibeVector> | null, c: WavesConfig = WAVES): Motion {
  const f = (x: number | null | undefined) => clamp01(x ?? 0.5);
  const m = c.motion;
  const at = (r: Range, t: number) => lerp(r[0], r[1], clamp01(t));
  const sw = m.speed_w.arousal + m.speed_w.energy || 1;
  const ww = m.warp_w.energy + m.warp_w.aggression || 1;
  return {
    speed: at(c.speed, (f(v?.arousal) * m.speed_w.arousal + f(v?.energy) * m.speed_w.energy) / sw),
    warpBase: at(
      m.warp_base,
      (f(v?.energy) * m.warp_w.energy + f(v?.aggression) * m.warp_w.aggression) / ww,
    ),
    beatGain: at(m.beat_gain, f(v?.danceability)),
    sparkleGain: at(m.sparkle_gain, 1 - f(v?.acousticness)),
    grain: at(m.grain, f(v?.acousticness)),
  };
}

/** The look `k` (0…1 of the transition) of the way from `a` to `b`, cosine-eased (Butterchurn's
 * blend, `01a` §B.2). */
export function blendLook(a: Look, b: Look, k: number): Look {
  if (k >= 1) return b;
  const e = 0.5 - 0.5 * Math.cos(Math.PI * clamp01(k));
  const mix = (x: number, y: number) => x + (y - x) * e;
  const palette = b.palette.map((col, i) =>
    col.map((v, j) => mix(a.palette[i]?.[j] ?? v, v)),
  ) as WavesPalette;
  const m = {} as Motion;
  for (const key of Object.keys(b.motion) as (keyof Motion)[]) {
    m[key] = mix(a.motion[key], b.motion[key]);
  }
  return { palette, motion: m };
}

/** The look of a track from what `viz_colors` returned. */
export function lookOf(p: PaletteInput, c: WavesConfig = WAVES): Look {
  return { palette: buildPalette(p, c), motion: motionOf(p.colors, c) };
}
