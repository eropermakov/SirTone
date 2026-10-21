import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useMemo, useRef, useState } from "react";
import { commands } from "@/shared/ipc/bindings";
import { useWeather } from "@/shared/ipc/context";
import { useEventOffer } from "@/shared/ipc/events";
import { getSettings, onSettingsChanged } from "@/shared/ipc/settings";
import { FxEngine, fxStats } from "@/shared/viz/fx";
import { type FxMode, type FxPlan, planFx } from "@/shared/viz/fxPlan";
import { useVizEnv } from "./useVizEnv";

type FxPrefs = { mode: FxMode; manual: string | null; staticWhenWeak: boolean };

/** Diagnostics for the smoke and the S-19 measurements: stats, and a forced plan. */
type FxDebug = {
  stats: typeof fxStats;
  force: (p: FxPlan | null | undefined) => void;
  probe: (n?: number) => Promise<{ ms: number; method: string }>;
};

/**
 * The live-effects layer (T4.20, FR-FX-001…007): only behind «Моя волна» and in the night vibe
 * (the visualiser mounts it), over the visualisation and under the scrim. Disposed with the
 * screen (its GL context goes too); paused with the visualiser (minimised, covered, tray).
 */
export function FxLayer() {
  const canvas = useRef<HTMLCanvasElement>(null);
  const engine = useRef<FxEngine | null>(null);
  const { prefs, reduced, power, effectiveLevel } = useVizEnv();
  const [fx, setFx] = useState<FxPrefs | null>(null);
  const [season, setSeason] = useState<string | null>(null);
  const [forced, setForced] = useState<FxPlan | null | undefined>(undefined);
  const [weatherSrc, setWeatherSrc] = useState<{
    effect: string | null;
    event: string | null;
    drift: number;
    intensity: number;
  }>({ effect: null, event: null, drift: 0, intensity: 0 });
  const eventOffer = useEventOffer();
  const weatherNow = useWeather();
  const [failed, setFailed] = useState(false);

  // biome-ignore lint/correctness/useExhaustiveDependencies: a weather or event update is the trigger itself.
  useEffect(() => {
    let alive = true;
    let off: (() => void) | undefined;
    const load = () => {
      getSettings().then((d) => {
        const f = (d.fx ?? {}) as {
          mode?: FxMode;
          manual?: string | null;
          static_when_weak?: boolean;
        };
        if (alive)
          setFx({
            mode: f.mode ?? "weather",
            manual: f.manual ?? null,
            staticWhenWeak: f.static_when_weak ?? false,
          });
      });
      // The season's effect (summer nights — fireflies) and the real weather's (T5.15), again
      // every hour and on every weather update.
      if (isTauri())
        commands
          .fxSources()
          .then((e) => {
            if (!alive) return;
            setSeason(e.season);
            setWeatherSrc({
              effect: e.weather,
              event: e.event,
              drift: e.drift ?? 0,
              intensity: e.intensity ?? 0,
            });
          })
          .catch(() => {});
    };
    load();
    const hourly = window.setInterval(load, 60 * 60 * 1000);
    onSettingsChanged(load).then((u) => {
      if (alive) off = u;
      else u();
    });
    const w = window as unknown as { __sirtoneFx?: FxDebug };
    w.__sirtoneFx = {
      stats: fxStats,
      force: setForced,
      probe: (n) => engine.current?.probeGpu(n) ?? Promise.resolve({ ms: -1, method: "none" }),
    };
    return () => {
      alive = false;
      window.clearInterval(hourly);
      off?.();
    };
  }, [weatherNow, eventOffer]);

  const plan = useMemo(() => {
    if (forced !== undefined) return forced;
    if (!prefs || !fx) return null;
    return planFx({
      mode: fx.mode,
      manual: fx.manual,
      seasonEffect: season,
      eventEffect: weatherSrc.event,
      weatherEffect: weatherSrc.effect,
      weatherDrift: weatherSrc.drift,
      weatherIntensity: weatherSrc.intensity,
      level: effectiveLevel,
      milkdrop: prefs.mode === "milkdrop",
      reduced,
      staticWhenWeak: fx.staticWhenWeak,
    });
  }, [forced, prefs, fx, season, weatherSrc, effectiveLevel, reduced]);

  const show = !!plan && !failed;
  const latest = useRef(plan);
  latest.current = plan;

  // The engine lives with the canvas.
  useEffect(() => {
    const el = canvas.current;
    if (!show || !el) return;
    const e = new FxEngine(el, { reducedMotion: reduced, onLost: () => setFailed(true) });
    if (!e.init()) {
      setFailed(true);
      return;
    }
    engine.current = e;
    e.setPlan(latest.current);
    const onResize = () => e.resize();
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      e.dispose();
      engine.current = null;
    };
  }, [show, reduced]);

  useEffect(() => {
    engine.current?.setPlan(plan);
  }, [plan]);

  useEffect(() => {
    const e = engine.current;
    if (!e) return;
    if (power.paused) e.pause();
    else e.resume();
  }, [power.paused]);

  if (!show) return null;
  return (
    <canvas
      ref={canvas}
      aria-hidden
      data-testid="fx"
      className="pointer-events-none absolute inset-0 size-full"
    />
  );
}
