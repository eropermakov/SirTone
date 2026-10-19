import { useEffect, useRef, useState } from "react";
import { getSettings, onSettingsChanged, type SettingsDoc } from "@/shared/ipc/settings";
import { onVizPower, startViz, vizColors } from "@/shared/ipc/viz";
import { fallbackColors, lookOf, type PaletteInput, WAVES } from "@/shared/viz/palette";
import { WavesEngine, wavesStats } from "@/shared/viz/waves";

type Prefs = { level: number; followAccent: boolean; minimal: boolean };

const prefsOf = (doc: SettingsDoc): Prefs => {
  const perf = (doc.performance ?? {}) as { level?: number };
  const viz = (doc.viz ?? {}) as { follow_theme_accent?: boolean; minimal?: boolean };
  return {
    level: perf.level ?? 3,
    followAccent: viz.follow_theme_accent ?? true,
    minimal: viz.minimal ?? false,
  };
};

const REDUCED = "(prefers-reduced-motion: reduce)";

/** The engine on screen, for pulses from the wave screen's own buttons (like). */
let active: WavesEngine | null = null;
export function pulseWaves(kind: "like" | "click"): void {
  active?.pulse(kind);
}

/**
 * The «Моя волна» background (T4.2, `06` §7.2, `DESIGN.md` §9): the «Волны» shader behind the
 * player, fed by the visualiser frames while it is on screen and the window is visible. Level 0,
 * no WebGL2 or a lost context — the blurred gradient of the cover's colours instead. Under
 * `prefers-reduced-motion` one still frame per track, no pulses. An `artwork-scrim` keeps the
 * alabaster text readable over any colour; raw colours never reach the controls.
 */
export function WavesBackground({
  trackId,
  cover,
}: {
  trackId: number | null;
  cover: string | null;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const engine = useRef<WavesEngine | null>(null);
  const [prefs, setPrefs] = useState<Prefs | null>(null);
  const [failed, setFailed] = useState(false);
  const [input, setInput] = useState<PaletteInput | null>(null);
  const [reduced, setReduced] = useState(() => window.matchMedia(REDUCED).matches);
  const [power, setPower] = useState({ paused: false, battery: false });
  const pausedRef = useRef(false);
  pausedRef.current = power.paused;

  useEffect(() => {
    const m = window.matchMedia(REDUCED);
    const on = () => setReduced(m.matches);
    m.addEventListener("change", on);
    let alive = true;
    let unlisten: (() => void) | undefined;
    const load = () => getSettings().then((d) => alive && setPrefs(prefsOf(d)));
    load();
    let unPower: (() => void) | undefined;
    onVizPower((p) => alive && setPower(p)).then((u) => {
      if (alive) unPower = u;
      else u();
    });
    onSettingsChanged(load).then((u) => {
      if (alive) unlisten = u;
      else u();
    });
    return () => {
      alive = false;
      m.removeEventListener("change", on);
      unlisten?.();
      unPower?.();
    };
  }, []);

  // The track's colours (cover palette, vibe, collection) — once per track.
  useEffect(() => {
    if (!prefs) return;
    let alive = true;
    const accent =
      getComputedStyle(document.documentElement).getPropertyValue("--accent-fill").trim() ||
      "#888888";
    const base = { accent, followAccent: prefs.followAccent, minimal: prefs.minimal };
    setInput((cur) => cur ?? { ...base, colors: null });
    vizColors(trackId, cover).then((colors) => {
      if (alive) setInput({ ...base, colors });
    });
    return () => {
      alive = false;
    };
  }, [trackId, cover, prefs]);

  const gl = !!prefs && prefs.level > 0 && !failed;
  // On battery one level lower (FR-VIZ-008), never below 1.
  const level = Math.max(1, (prefs?.level ?? 3) - (power.battery ? 1 : 0));
  const quality = WAVES.quality[String(level) as "1" | "2" | "3" | "4"];
  const minimal = !!prefs && (prefs.minimal || quality.minimal);
  // What a newly created engine starts with.
  const latest = useRef({ quality, minimal, input });
  latest.current = { quality, minimal, input };

  // The engine: created with the canvas, fed while visible.
  useEffect(() => {
    const el = canvas.current;
    if (!gl || !el) return;
    const e = new WavesEngine(el, { reducedMotion: reduced, onLost: () => setFailed(true) });
    if (!e.init()) {
      setFailed(true);
      return;
    }
    engine.current = e;
    active = e;
    const now = latest.current;
    e.setQuality({ ...now.quality, minimal: now.minimal });
    if (now.input) {
      e.setVibe(lookOf({ ...now.input, minimal: now.minimal }));
    }
    (window as unknown as { __sirtoneWaves?: unknown }).__sirtoneWaves = {
      stats: wavesStats,
      sample: () => e.sample(),
    };
    let stop: (() => void) | null = null;
    const run = () => {
      if (document.hidden) {
        stop?.();
        stop = null;
        e.pause();
        return;
      }
      if (!reduced && !stop) stop = startViz((f) => e.setAudioFrame(f));
      if (!pausedRef.current) e.resume();
    };
    run();
    document.addEventListener("visibilitychange", run);
    const ro = new ResizeObserver(() => e.resize());
    ro.observe(el);
    const host = el.parentElement?.parentElement;
    const toEngine = (ev: PointerEvent, kind: "move" | "down") => {
      const r = el.getBoundingClientRect();
      if (r.width === 0 || r.height === 0) return;
      e.pointer({ x: (ev.clientX - r.left) / r.width, y: (ev.clientY - r.top) / r.height, kind });
    };
    const move = (ev: PointerEvent) => toEngine(ev, "move");
    const down = (ev: PointerEvent) => toEngine(ev, "down");
    host?.addEventListener("pointermove", move);
    host?.addEventListener("pointerdown", down);
    return () => {
      document.removeEventListener("visibilitychange", run);
      host?.removeEventListener("pointermove", move);
      host?.removeEventListener("pointerdown", down);
      ro.disconnect();
      stop?.();
      e.dispose();
      if (active === e) active = null;
      engine.current = null;
    };
  }, [gl, reduced]);

  useEffect(() => {
    engine.current?.setQuality({ ...quality, minimal });
  }, [quality, minimal]);

  // Not seen (minimised, covered, a full-screen game): no drawing; the feed idles in Rust.
  useEffect(() => {
    const e = engine.current;
    if (!e) return;
    if (power.paused) e.pause();
    else if (!document.hidden) e.resume();
  }, [power.paused]);

  useEffect(() => {
    if (!input) return;
    engine.current?.setVibe(lookOf({ ...input, minimal }));
  }, [input, minimal]);

  const colors = input ? fallbackColors(input) : null;
  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
      {gl ? (
        <canvas ref={canvas} data-testid="waves" className="absolute inset-0 size-full" />
      ) : (
        colors && (
          <div
            data-testid="waves-fallback"
            className="absolute -inset-1/5"
            style={{
              background: `radial-gradient(40% 45% at 30% 35%, ${colors[0]}, transparent), radial-gradient(45% 40% at 70% 65%, ${colors[1] ?? colors[0]}, transparent), ${colors[2] ?? colors[1] ?? colors[0]}`,
              filter: `blur(${WAVES.fallback.blur_px}px)`,
              animation: `waves-drift ${WAVES.fallback.drift_s}s ease-in-out infinite`,
            }}
          />
        )
      )}
      <div className="absolute inset-0 bg-(--artwork-scrim)" />
    </div>
  );
}
