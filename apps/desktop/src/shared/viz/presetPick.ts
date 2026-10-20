import cfg from "../../../../../assets/viz/milkdrop.json";
import type { VizColors } from "../ipc/bindings";
import { vibeColor } from "./palette";

/**
 * Which MilkDrop preset suits the track (T4.3, FR-VIZ-004, `06` §7.3): the vibe becomes a target
 * in the presets' descriptor space (hue, motion, reactivity, brightness — measured by
 * `tools/preset-indexer`), the closest `top_k` kept presets are the candidates, one is taken at
 * random, never one of the last `recent`. ♥ (rating +1) draws a preset closer, ✕ (−1) removes it
 * (Kodi «Rate», `01a` §B.2). Numbers — `assets/viz/milkdrop.json`.
 */
export type MilkdropConfig = {
  preset_s: number;
  blend_s: number;
  top_k: number;
  recent: number;
  like: number;
  weights: { hue: number; motion: number; react: number; bright: number };
  quality: Record<
    "1" | "2" | "3" | "4",
    { fps: number; mesh: [number, number]; pixel_ratio: number; texture_ratio: number }
  >;
};
export const MILKDROP = cfg as unknown as MilkdropConfig;

export type PresetDesc = {
  name: string;
  author: string;
  hue: number;
  sat: number;
  bright: number;
  motion: number;
  react: number;
  chaos: number;
};

/** A target: hue in degrees, the rest as ranks 0…1 among the kept presets. */
export type Target = { hue: number; motion: number; react: number; bright: number };

const clamp01 = (x: number) => Math.min(1, Math.max(0, x));

/** The vibe → the target. Unknown features sit in the middle. */
export function targetOf(c: VizColors | null): Target {
  const f = (x: number | null | undefined) => clamp01(x ?? 0.5);
  return {
    hue: vibeColor(f(c?.valence), f(c?.arousal)).h,
    motion: 0.6 * f(c?.arousal) + 0.4 * f(c?.energy),
    react: f(c?.danceability),
    bright: 0.25 + 0.6 * f(c?.valence),
  };
}

type Ranked = PresetDesc & { rMotion: number; rReact: number; rBright: number };

/** Descriptors → ranks (the measured scales differ; ranks make them comparable). */
export function rank(index: PresetDesc[]): Ranked[] {
  const n = Math.max(1, index.length - 1);
  const ranks = (key: "motion" | "react" | "bright") => {
    const order = [...index].sort((a, b) => a[key] - b[key]).map((p) => p.name);
    return new Map(order.map((name, i) => [name, i / n]));
  };
  const m = ranks("motion");
  const r = ranks("react");
  const b = ranks("bright");
  return index.map((p) => ({
    ...p,
    rMotion: m.get(p.name) ?? 0.5,
    rReact: r.get(p.name) ?? 0.5,
    rBright: b.get(p.name) ?? 0.5,
  }));
}

export function distance(p: Ranked, t: Target, c: MilkdropConfig = MILKDROP): number {
  const dh = Math.abs(((p.hue - t.hue + 540) % 360) - 180) / 180;
  const w = c.weights;
  // A grey preset has no hue to match.
  const hueW = w.hue * clamp01(p.sat * 2);
  return (
    hueW * dh +
    w.motion * Math.abs(p.rMotion - t.motion) +
    w.react * Math.abs(p.rReact - t.react) +
    w.bright * Math.abs(p.rBright - t.bright)
  );
}

export function pick(
  ranked: Ranked[],
  t: Target,
  opts: {
    recent: string[];
    ratings: Map<string, number>;
    rand?: () => number;
    /** «Только мои пресеты»: only the ♥ ones (if there are any). */
    onlyLiked?: boolean;
  },
  c: MilkdropConfig = MILKDROP,
): string | null {
  const recent = new Set(opts.recent.slice(-c.recent));
  const liked = (p: Ranked) => (opts.ratings.get(p.name) ?? 0) > 0;
  const onlyLiked = !!opts.onlyLiked && ranked.some(liked);
  const cands = ranked
    .filter((p) => (opts.ratings.get(p.name) ?? 0) >= 0 && (!onlyLiked || liked(p)))
    .map((p) => ({
      name: p.name,
      d: distance(p, t, c) * ((opts.ratings.get(p.name) ?? 0) > 0 ? c.like : 1),
    }))
    .sort((a, b) => a.d - b.d);
  const fresh = cands.filter((x) => !recent.has(x.name));
  const pool = (fresh.length > 0 ? fresh : cands).slice(0, c.top_k);
  if (pool.length === 0) return null;
  const r = opts.rand ?? Math.random;
  return pool[Math.min(pool.length - 1, Math.floor(r() * pool.length))]?.name ?? null;
}
