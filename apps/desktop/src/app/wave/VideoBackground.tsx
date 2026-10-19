import { useEffect, useRef, useState } from "react";
import { videoUrl, vizColors } from "@/shared/ipc/viz";
import { useVizEnv } from "./useVizEnv";
import { clipFor, moodOf, useVideoControl, videoControl } from "./videoControl";

/**
 * The «Видео» background (T4.4, FR-VIZ-005, `06` §7.4 — Spotify Canvas: a short silent loop,
 * `01a` §B.3): a clip of the track's mood (or «любое»), muted, looping, covering the screen.
 * Paused while the window is hidden or under `prefers-reduced-motion` (a still frame). No clips —
 * `onEmpty`, the caller shows «Волны».
 */
export function VideoBackground({
  trackId,
  cover,
  scrim = true,
  onEmpty,
}: {
  trackId: number | null;
  cover: string | null;
  scrim?: boolean;
  onEmpty: () => void;
}) {
  const video = useRef<HTMLVideoElement>(null);
  const { clips } = useVideoControl();
  const { reduced, power } = useVizEnv();
  const [mood, setMood] = useState("any");
  const emptyRef = useRef(onEmpty);
  emptyRef.current = onEmpty;

  useEffect(() => {
    let alive = true;
    vizColors(trackId, cover).then((c) => {
      if (alive) setMood(moodOf(c?.valence ?? null, c?.arousal ?? null));
    });
    return () => {
      alive = false;
    };
  }, [trackId, cover]);

  const clip = clipFor(clips, mood, trackId ?? 0);
  useEffect(() => {
    if (!clip) emptyRef.current();
    videoControl.set({ current: clip?.id ?? "" });
  }, [clip]);

  const clipId = clip?.id;
  useEffect(() => {
    const v = video.current;
    if (!v || !clipId) return;
    const sync = () => {
      if (reduced || power.paused || document.hidden) v.pause();
      else void v.play().catch(() => {});
    };
    sync();
    document.addEventListener("visibilitychange", sync);
    return () => document.removeEventListener("visibilitychange", sync);
  }, [reduced, power.paused, clipId]);

  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
      {clip && (
        <video
          ref={video}
          key={clip.id}
          data-testid="viz-video"
          src={videoUrl(clip.id)}
          muted
          loop
          playsInline
          autoPlay={!reduced}
          className="absolute inset-0 size-full object-cover"
        />
      )}
      {scrim && <div className="absolute inset-0 bg-(--artwork-scrim)" />}
    </div>
  );
}
