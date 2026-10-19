import { useEffect, useRef, useState } from "react";
import type { VizPower } from "@/shared/ipc/bindings";
import { getSettings, onSettingsChanged, type SettingsDoc } from "@/shared/ipc/settings";
import { onVizPower } from "@/shared/ipc/viz";

export type VizMode = "waves" | "milkdrop" | "video";

export type VizPrefs = {
  level: number;
  mode: VizMode;
  followAccent: boolean;
  minimal: boolean;
};

const prefsOf = (doc: SettingsDoc): VizPrefs => {
  const perf = (doc.performance ?? {}) as { level?: number };
  const viz = (doc.viz ?? {}) as {
    mode?: VizMode;
    follow_theme_accent?: boolean;
    minimal?: boolean;
  };
  return {
    level: perf.level ?? 3,
    mode: viz.mode ?? "waves",
    followAccent: viz.follow_theme_accent ?? true,
    minimal: viz.minimal ?? false,
  };
};

const REDUCED = "(prefers-reduced-motion: reduce)";

/**
 * What every visualiser follows (T4.2–T4.6): the settings (level, mode, accent, minimal), the
 * reduced-motion preference and the power state from the feed thread (paused / on battery).
 * `effectiveLevel` is one lower on battery (FR-VIZ-008), never below 1 while a visual runs.
 */
export function useVizEnv() {
  const [prefs, setPrefs] = useState<VizPrefs | null>(null);
  const [reduced, setReduced] = useState(() => window.matchMedia(REDUCED).matches);
  const [power, setPower] = useState<VizPower>({ paused: false, battery: false });
  const pausedRef = useRef(false);
  pausedRef.current = power.paused;

  useEffect(() => {
    const m = window.matchMedia(REDUCED);
    const on = () => setReduced(m.matches);
    m.addEventListener("change", on);
    let alive = true;
    const offs: (() => void)[] = [];
    const keep = (u: () => void) => {
      if (alive) offs.push(u);
      else u();
    };
    const load = () => getSettings().then((d) => alive && setPrefs(prefsOf(d)));
    load();
    onSettingsChanged(load).then(keep);
    onVizPower((p) => alive && setPower(p)).then(keep);
    return () => {
      alive = false;
      m.removeEventListener("change", on);
      for (const u of offs) u();
    };
  }, []);

  const effectiveLevel = Math.max(1, (prefs?.level ?? 3) - (power.battery ? 1 : 0));
  return { prefs, reduced, power, pausedRef, effectiveLevel };
}
