import { useCallback, useEffect, useRef, useState } from "react";
import { presetRate, presetRatings, startViz, vizColors } from "@/shared/ipc/viz";
import { MilkDropEngine, milkdropStats } from "@/shared/viz/milkdrop";
import { MILKDROP, type PresetDesc, pick, rank, targetOf } from "@/shared/viz/presetPick";
import index from "../../../../../assets/viz/milkdrop/index.json";
import { milkdropControl } from "./milkdropControl";
import { useVizEnv } from "./useVizEnv";

const RANKED = rank((index as { presets: PresetDesc[] }).presets);

/**
 * The «MilkDrop» background (T4.3, FR-VIZ-004, `06` §7.3): Butterchurn behind the player, a
 * preset chosen by the vibe on every track change, the next one every `preset_s` (3 s blend)
 * unless locked; ♥/✕ rate the preset on screen (Kodi Lock/Rate). Fed by the visualiser frames;
 * paused when unseen (T4.6). Failing (no WebGL2, lost context) — `onFail`, the caller falls back
 * to «Волны».
 */
export function MilkDropBackground({
  trackId,
  cover,
  scrim = true,
  onFail,
}: {
  trackId: number | null;
  cover: string | null;
  scrim?: boolean;
  onFail: () => void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const engine = useRef<MilkDropEngine | null>(null);
  const { pausedRef, power, effectiveLevel } = useVizEnv();
  const [ready, setReady] = useState(false);
  const target = useRef(targetOf(null));
  const recent = useRef<string[]>([]);
  const ratings = useRef(new Map<string, number>());

  const failRef = useRef(onFail);
  failRef.current = onFail;

  // Reads refs only: stable for the whole mount.
  const next = useCallback(() => {
    const e = engine.current;
    if (!e) return;
    // A preset that fails to load is skipped for this session and another one is taken.
    for (let attempt = 0; attempt < 5; attempt++) {
      const name = pick(RANKED, target.current, {
        recent: recent.current,
        ratings: ratings.current,
      });
      if (!name) return;
      recent.current = [...recent.current, name].slice(-MILKDROP.recent);
      try {
        e.show(name);
      } catch {
        ratings.current.set(name, -1);
        continue;
      }
      milkdropControl.set({ preset: name, rating: ratings.current.get(name) ?? 0 });
      return;
    }
  }, []);

  // The engine and the feed.
  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    let alive = true;
    let stop: (() => void) | null = null;
    const e = new MilkDropEngine(el, { onLost: () => failRef.current() });
    const run = () => {
      if (!alive) return;
      if (document.hidden) {
        stop?.();
        stop = null;
        e.pause();
        return;
      }
      if (!stop) stop = startViz((f) => e.setAudioFrame(f));
      if (!pausedRef.current) e.resume();
    };
    const ro = new ResizeObserver(() => e.resize());
    e.init().then(async (ok) => {
      if (!alive) return;
      if (!ok) {
        failRef.current();
        return;
      }
      engine.current = e;
      (window as unknown as { __sirtoneMilkdrop?: unknown }).__sirtoneMilkdrop = {
        stats: milkdropStats,
        count: e.names().length,
        control: () => milkdropControl.get(),
      };
      for (const r of await presetRatings()) ratings.current.set(r.name, r.rating);
      e.resize();
      ro.observe(el);
      next();
      run();
      setReady(true);
    });
    document.addEventListener("visibilitychange", run);
    milkdropControl.bind({
      rate: (rating) => {
        const name = milkdropControl.get().preset;
        if (!name) return;
        ratings.current.set(name, rating);
        void presetRate(name, rating);
        milkdropControl.set({ rating });
        // ✕ — away with it now.
        if (rating < 0) next();
      },
      skip: next,
    });
    return () => {
      alive = false;
      document.removeEventListener("visibilitychange", run);
      ro.disconnect();
      stop?.();
      e.dispose();
      engine.current = null;
      milkdropControl.bind(null);
    };
  }, [next, pausedRef]);

  useEffect(() => {
    if (!ready) return;
    const q = MILKDROP.quality[String(Math.min(4, effectiveLevel)) as "1" | "2" | "3" | "4"];
    engine.current?.setQuality(q);
  }, [effectiveLevel, ready]);

  useEffect(() => {
    const e = engine.current;
    if (!e || !ready) return;
    if (power.paused) e.pause();
    else if (!document.hidden) e.resume();
  }, [power.paused, ready]);

  // A new track — a preset by its vibe.
  useEffect(() => {
    let alive = true;
    vizColors(trackId, cover).then((c) => {
      if (!alive) return;
      target.current = targetOf(c);
      if (ready) next();
    });
    return () => {
      alive = false;
    };
  }, [trackId, cover, ready, next]);

  // Rotation: the next preset every `preset_s` unless locked.
  useEffect(() => {
    if (!ready) return;
    const id = window.setInterval(() => {
      if (!milkdropControl.get().locked && !power.paused) next();
    }, MILKDROP.preset_s * 1000);
    return () => window.clearInterval(id);
  }, [ready, power.paused, next]);

  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
      <canvas ref={canvas} data-testid="milkdrop" className="absolute inset-0 size-full" />
      {scrim && <div className="absolute inset-0 bg-(--artwork-scrim)" />}
    </div>
  );
}
