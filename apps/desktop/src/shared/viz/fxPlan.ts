import FX_JSON from "../../../../../assets/viz/fx.json";
import THEMES from "../../../../../assets/wave/themes.json";
import { hexToRgb } from "../lib/color";
import { tryFlash } from "./flashBudget";

/**
 * What the live-effects layer draws (T4.20, FR-FX-001…007, `14` §13, supplement §31): pure
 * decisions — which effect, how many particles at what FPS for the performance level, the still
 * version, the storm flash clock — all numbers from `assets/viz/fx.json`.
 */
export const FX = FX_JSON as unknown as FxConfig;

export const FX_TYPES = [
  "snow",
  "rain",
  "fog",
  "storm",
  "clouds",
  "motes",
  "fireflies",
  "leaves",
  "petals",
] as const;
export type FxType = (typeof FX_TYPES)[number];
export type FxMode = "weather" | "season" | "manual" | "off";
export type FxShape = "dot" | "streak" | "leaf" | "glow" | "noise";

export type FxTypeCfg = {
  shape: FxShape;
  density?: number;
  size?: [number, number];
  length?: [number, number];
  fall?: [number, number];
  sway?: number;
  sway_hz?: [number, number];
  spin?: number;
  wind: number;
  alpha: number;
  soft?: number;
  blink?: boolean;
  flash?: boolean;
  speed?: number;
  scale?: number;
  shadow?: boolean;
  palette: [string, number | "*"];
};

export type FxConfig = {
  budget: Record<string, { count: number; fps: number; planes: number }>;
  milkdrop_halve_max_level: number;
  static_count: number;
  season_density: number;
  transition_ms: number;
  breath: { swirl: number; beat_decay: number; bass_alpha: number };
  flash: { min_gap_s: number; max_gap_s: number; ms: number; lift: number };
  types: Record<FxType, FxTypeCfg>;
  noise_scale: number;
};

export const isFxType = (s: string | null | undefined): s is FxType =>
  !!s && (FX_TYPES as readonly string[]).includes(s);

/** The level's budget (§13.3); with MilkDrop at level ≤ 3 — half the particles. */
export function budget(
  level: number,
  milkdrop: boolean,
  c: FxConfig = FX,
): { count: number; fps: number; planes: number } {
  const b = c.budget[String(Math.max(0, Math.min(4, level)))] ?? { count: 0, fps: 0, planes: 1 };
  const half = milkdrop && level <= c.milkdrop_halve_max_level;
  return { ...b, count: half ? Math.floor(b.count / 2) : b.count };
}

export type FxInput = {
  mode: FxMode;
  manual: string | null;
  /** «По сезону» right now (Rust, by the clock). */
  seasonEffect: string | null;
  /** The real weather's effect; `null` — no weather or older than 3 h (T5.15). */
  weatherEffect: string | null;
  level: number;
  milkdrop: boolean;
  reduced: boolean;
  staticWhenWeak: boolean;
};

export type FxPlan = {
  type: FxType;
  count: number;
  fps: number;
  planes: number;
  /** One still frame, no animation (level 1 switch, reduced motion). */
  still: boolean;
  flash: boolean;
};

/** `null` — nothing on screen. */
export function planFx(p: FxInput, c: FxConfig = FX): FxPlan | null {
  if (p.mode === "off" || p.level <= 0) return null;
  const fromSeason = isFxType(p.seasonEffect) ? p.seasonEffect : null;
  let type: FxType | null;
  let decorative = false;
  if (p.mode === "manual") type = isFxType(p.manual) ? p.manual : fromSeason;
  else if (p.mode === "weather" && isFxType(p.weatherEffect)) type = p.weatherEffect;
  else {
    // «По сезону», or the weather is unknown (AT-16): decorative and sparser.
    type = fromSeason;
    decorative = true;
  }
  if (!type) return null;
  const t = c.types[type];
  const weak = p.level === 1;
  if (weak && !p.staticWhenWeak) return null;
  const still = weak || p.reduced;
  const b = weak ? { count: c.static_count, fps: 0, planes: 1 } : budget(p.level, p.milkdrop, c);
  const density = (t.density ?? 1) * (decorative ? c.season_density : 1);
  return {
    type,
    count: t.shape === "noise" ? 0 : Math.max(1, Math.round(b.count * density)),
    fps: still ? 0 : b.fps,
    planes: b.planes,
    still,
    flash: !!t.flash && !still,
  };
}

/** The type's colours (1–3) from the season palettes, as 0…1 RGB. */
export function fxColors(type: FxType, c: FxConfig = FX): [number, number, number][] {
  const [name, idx] = c.types[type].palette;
  const pal = (THEMES.seasons as Record<string, { viz: string[] }>)[name]?.viz ?? ["#ffffff"];
  const hexes = idx === "*" ? pal.slice(0, 3) : [pal[idx] ?? pal[0] ?? "#ffffff"];
  return hexes.map((h) => {
    const { r, g, b } = hexToRgb(h);
    return [r, g, b];
  });
}

/**
 * The storm's soft flashes (FR-FX-006, WCAG 2.3.1): never closer than `min_gap_s` — at most
 * one in 10 s — a smooth rise and fall over `ms`. `rand` is injectable for tests.
 */
export class FlashClock {
  private next: number;
  private at = Number.NEGATIVE_INFINITY;

  constructor(
    now: number,
    private rand: () => number = Math.random,
    private c: FxConfig["flash"] = FX.flash,
  ) {
    this.next = now + this.gap();
  }

  private gap(): number {
    const { min_gap_s: lo, max_gap_s: hi } = this.c;
    return (lo + (hi - lo) * this.rand()) * 1000;
  }

  /** The flash brightness lift now (0 — none). */
  level(now: number): number {
    if (now >= this.next) {
      // The screen's one budget (shared with the «Волны» lift): too soon — try again later.
      if (tryFlash(now, this.c.min_gap_s * 1000)) this.at = now;
      this.next = now + Math.max(this.c.min_gap_s * 1000, this.gap());
    }
    const k = (now - this.at) / this.c.ms;
    return k >= 0 && k < 1 ? this.c.lift * Math.sin(Math.PI * k) : 0;
  }
}
