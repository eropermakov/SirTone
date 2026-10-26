import { useEffect, useState } from "react";
import { useFocusStatus } from "@/shared/ipc/focus";
import { useSleepStatus } from "@/shared/ipc/sleep";
import { WAVES } from "@/shared/viz/palette";
import { wavesMood } from "@/shared/viz/waves";
import { FxLayer } from "./FxLayer";
import { MilkDropBackground } from "./MilkDropBackground";
import { useVizEnv } from "./useVizEnv";
import { VideoBackground } from "./VideoBackground";
import { useVideoControl } from "./videoControl";
import { WavesBackground } from "./WavesBackground";

/**
 * The visualisation behind «Моя волна» (FR-VIZ-001): the mode from the settings, switched on the
 * fly. Only one engine and one GL context live at a time. «MilkDrop» and «Видео» are heavy: only
 * from «Мощное» up (`06` §6, the level table; D-118). «MilkDrop» needs motion and WebGL2: under
 * `prefers-reduced-motion`, below «Мощное» or when it fails, «Волны» stand in (their still frame
 * or gradient); so do they for «Видео» until the person has dropped a clip in.
 */
/** «Мощное»: MilkDrop and the video background start here (`06` §6). */
const HEAVY_LEVEL = 3;

export function VizBackground({
  trackId,
  cover,
  scrim = true,
}: {
  trackId: number | null;
  cover: string | null;
  scrim?: boolean;
}) {
  const { prefs, reduced } = useVizEnv();
  const { s: sleep } = useSleepStatus();
  const focusWork = useFocusStatus().s.phase === "work";
  const fading = sleep.phase === "fading" || sleep.phase === "final";
  const dim = fading ? WAVES.sleep.dim : 0;
  // Slower while the timer fades: ×0.4, ×0.2 in the final.
  useEffect(() => {
    wavesMood.speed =
      sleep.phase === "final"
        ? WAVES.sleep.speed_final
        : fading
          ? WAVES.sleep.speed
          : focusWork
            ? WAVES.focus.speed
            : 1;
  }, [sleep.phase, fading, focusWork]);
  const [failed, setFailed] = useState(false);
  const mode = prefs?.mode ?? "waves";
  useEffect(() => {
    if (mode) setFailed(false);
  }, [mode]);
  // «Видео» with no clips yet (the person drops them in) — «Волны» meanwhile.
  const [noClips, setNoClips] = useState(false);
  const { clips } = useVideoControl();
  useEffect(() => {
    if (clips.length > 0) setNoClips(false);
  }, [clips]);
  if (!prefs) return null;
  let bg: React.ReactNode;
  const heavy = prefs.level >= HEAVY_LEVEL;
  const milkdrop = mode === "milkdrop" && heavy && !reduced && !failed;
  if (milkdrop) {
    bg = (
      <MilkDropBackground
        trackId={trackId}
        cover={cover}
        scrim={false}
        onFail={() => setFailed(true)}
      />
    );
  } else if (mode === "video" && heavy && !noClips) {
    bg = (
      <VideoBackground
        trackId={trackId}
        cover={cover}
        scrim={false}
        onEmpty={() => setNoClips(true)}
      />
    );
  } else {
    bg = <WavesBackground trackId={trackId} cover={cover} scrim={false} />;
  }
  // The live effects sit over the visualisation and under the scrim, so the text contrast
  // the scrim guarantees holds (T4.20).
  return (
    <>
      {bg}
      {/* Not over MilkDrop: its presets are scenes of their own, particles only clutter them
          (owner, 2026-10-20, D-105). */}
      {!milkdrop && <FxLayer />}
      {/* The sleep timer (FR-SLEEP-005, `06` §7.8): dimmer while it fades. */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 bg-bg-base transition-opacity duration-(--dur-theme)"
        style={{ opacity: dim }}
      />
      {scrim && (
        <div aria-hidden className="pointer-events-none absolute inset-0 bg-(--artwork-scrim)" />
      )}
    </>
  );
}
