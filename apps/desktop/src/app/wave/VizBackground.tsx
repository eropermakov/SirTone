import { useEffect, useState } from "react";
import { FxLayer } from "./FxLayer";
import { MilkDropBackground } from "./MilkDropBackground";
import { useVizEnv } from "./useVizEnv";
import { VideoBackground } from "./VideoBackground";
import { useVideoControl } from "./videoControl";
import { WavesBackground } from "./WavesBackground";

/**
 * The visualisation behind «Моя волна» (FR-VIZ-001): the mode from the settings, switched on the
 * fly. Only one engine and one GL context live at a time. «MilkDrop» needs motion and WebGL2:
 * under `prefers-reduced-motion`, on level 0 or when it fails, «Волны» stand in (their still
 * frame or gradient); so do they for «Видео» until the person has dropped a clip in.
 */
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
  if (mode === "milkdrop" && prefs.level > 0 && !reduced && !failed) {
    bg = (
      <MilkDropBackground
        trackId={trackId}
        cover={cover}
        scrim={false}
        onFail={() => setFailed(true)}
      />
    );
  } else if (mode === "video" && prefs.level > 0 && !noClips) {
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
      <FxLayer />
      {scrim && (
        <div aria-hidden className="pointer-events-none absolute inset-0 bg-(--artwork-scrim)" />
      )}
    </>
  );
}
