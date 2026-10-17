/**
 * Performance presets (T2.3, FR-PERF-001/002, docs/06 §6 — copy of Wallpaper Engine / Lively /
 * Chrome Energy Saver): one level 0–4 sets motion, blur and list animations; point switches
 * (`performance.overrides`) win over the level. Applied as one write to <html> (data attributes):
 * CSS scales the duration tokens, nothing animates per frame.
 */
export type PerfLevel = 0 | 1 | 2 | 3 | 4;

export type PerfProfile = {
  /** docs/06 §5.1 rule 4: 0 / 0.5 / 1 / 1.25 / 1.5. */
  motionScale: number;
  uiAnimations: boolean;
  blur: boolean;
  listAnimations: boolean;
};

const MOTION = [0, 0.5, 1, 1.25, 1.5] as const;

export function profile(level: number, overrides: Record<string, unknown> = {}): PerfProfile {
  const l = Math.min(4, Math.max(0, Math.round(level))) as PerfLevel;
  const base: PerfProfile = {
    motionScale: MOTION[l],
    uiAnimations: l > 0,
    blur: l >= 2,
    listAnimations: l >= 2,
  };
  const bool = (k: string, d: boolean) =>
    typeof overrides[k] === "boolean" ? (overrides[k] as boolean) : d;
  const p: PerfProfile = {
    uiAnimations: bool("ui_animations", base.uiAnimations),
    blur: bool("blur", base.blur),
    listAnimations: bool("list_animations", base.listAnimations),
    motionScale: base.motionScale,
  };
  if (!p.uiAnimations) p.motionScale = 0;
  else if (p.motionScale === 0) p.motionScale = 1;
  return p;
}

/** One assignment per change (docs/06 §5.1 rule 3). */
export function applyPerformance(p: PerfProfile): void {
  const root = document.documentElement;
  root.dataset.motion = String(p.motionScale);
  root.dataset.blur = p.blur ? "on" : "off";
  root.dataset.listAnimations = p.listAnimations ? "on" : "off";
}
