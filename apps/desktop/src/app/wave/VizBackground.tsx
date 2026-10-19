import { useEffect, useState } from "react";
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
  if (mode === "milkdrop" && prefs.level > 0 && !reduced && !failed) {
    return (
      <MilkDropBackground
        trackId={trackId}
        cover={cover}
        scrim={scrim}
        onFail={() => setFailed(true)}
      />
    );
  }
  if (mode === "video" && prefs.level > 0 && !noClips) {
    return (
      <VideoBackground
        trackId={trackId}
        cover={cover}
        scrim={scrim}
        onEmpty={() => setNoClips(true)}
      />
    );
  }
  return <WavesBackground trackId={trackId} cover={cover} scrim={scrim} />;
}
