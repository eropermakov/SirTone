import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { getSnapshot, onSnapshot } from "@/shared/ipc/playback";
import { startViz, vizColors } from "@/shared/ipc/viz";
import { TitlesEngine, type TrackMeta, titlesStats } from "@/shared/viz/titles/engine";
import { seedOf } from "@/shared/viz/titles/rng";
import { useVizEnv } from "./useVizEnv";

/**
 * The «Титры» background (FR-VIZ-012, D-122, `06` §7.8): kinetic typography cut to the track's
 * beat, drawn on one 2D canvas at 24 frames a second. It writes the track's own data — title,
 * artist, tempo, mood — as titles. Under `prefers-reduced-motion` the cards change with a short
 * fade, nothing flashes or glitches. No 2D context — `onFail`, «Волны» take over. Paused when the
 * window is not seen (T4.6).
 */
export function TitlesBackground({
  trackId,
  cover,
  onFail,
}: {
  trackId: number | null;
  cover: string | null;
  onFail: () => void;
}) {
  const { t } = useTranslation("shell");
  const canvas = useRef<HTMLCanvasElement>(null);
  const engine = useRef<TitlesEngine | null>(null);
  const { prefs, reduced, power, pausedRef, effectiveLevel } = useVizEnv();
  const [now, setNow] = useState<{ title: string | null; artist: string | null }>({
    title: null,
    artist: null,
  });
  const [vibe, setVibe] = useState<{ valence: number | null; arousal: number | null } | null>(null);
  const failRef = useRef(onFail);
  failRef.current = onFail;

  // The playing track's words, and where it is in the track (for the acts).
  useEffect(() => {
    let alive = true;
    let off: (() => void) | undefined;
    const take = (s: {
      title: string | null;
      artists: string | null;
      positionMs: number;
      durationMs: number | null;
    }) => {
      if (!alive) return;
      setNow((cur) =>
        cur.title === s.title && cur.artist === s.artists
          ? cur
          : { title: s.title, artist: s.artists },
      );
      engine.current?.setProgress(s.positionMs, s.durationMs);
    };
    getSnapshot().then(take);
    onSnapshot(take).then((u) => {
      if (alive) off = u;
      else u();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, []);

  // The vibe of the track: the mood word and the style «Авто».
  useEffect(() => {
    let alive = true;
    vizColors(trackId, cover).then((c) => {
      if (alive) setVibe(c ? { valence: c.valence, arousal: c.arousal } : null);
    });
    return () => {
      alive = false;
    };
  }, [trackId, cover]);

  const meta = useMemo<TrackMeta>(
    () => ({
      seed: seedOf(trackId, now.title),
      title: now.title,
      artist: now.artist,
      vibe,
      labels: {
        title: t("viz.titles.role.title"),
        artist: t("viz.titles.role.artist"),
        tempo: t("viz.titles.role.tempo"),
        mood: t("viz.titles.role.mood"),
        moods: {
          happy: t("viz.moods.happy"),
          energetic: t("viz.moods.energetic"),
          calm: t("viz.moods.calm"),
          sad: t("viz.moods.sad"),
        },
      },
    }),
    [trackId, now, vibe, t],
  );
  const latest = useRef({ meta, style: prefs?.titlesStyle ?? "auto", level: effectiveLevel });
  latest.current = { meta, style: prefs?.titlesStyle ?? "auto", level: effectiveLevel };

  // The engine lives with the canvas.
  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const e = new TitlesEngine(el, { reducedMotion: reduced });
    if (!e.init()) {
      failRef.current();
      return;
    }
    engine.current = e;
    const now0 = latest.current;
    e.setQuality(now0.level);
    e.setStyle(now0.style);
    e.setTrack(now0.meta);
    (window as unknown as { __sirtoneTitles?: unknown }).__sirtoneTitles = {
      stats: titlesStats,
      engine: e,
    };
    let stop: (() => void) | null = null;
    const run = () => {
      if (document.hidden) {
        stop?.();
        stop = null;
        e.pause();
        return;
      }
      if (!stop) stop = startViz((f) => e.setAudioFrame(f));
      if (!pausedRef.current) e.resume();
    };
    run();
    document.addEventListener("visibilitychange", run);
    const ro = new ResizeObserver(() => e.resize());
    ro.observe(el);
    return () => {
      document.removeEventListener("visibilitychange", run);
      ro.disconnect();
      stop?.();
      e.dispose();
      engine.current = null;
    };
  }, [reduced, pausedRef]);

  useEffect(() => engine.current?.setTrack(meta), [meta]);
  useEffect(() => engine.current?.setStyle(prefs?.titlesStyle ?? "auto"), [prefs?.titlesStyle]);
  useEffect(() => engine.current?.setQuality(effectiveLevel), [effectiveLevel]);

  useEffect(() => {
    const e = engine.current;
    if (!e) return;
    if (power.paused) e.pause();
    else if (!document.hidden) e.resume();
  }, [power.paused]);

  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
      <canvas ref={canvas} data-testid="titles" className="absolute inset-0 size-full bg-bg-base" />
    </div>
  );
}
