import { useEffect, useState } from "react";
import { getQueue, onQueueChanged } from "@/shared/ipc/playback";

/**
 * The playlist that is playing (`QueueView.playlist`) — for the sidebar bars. Read again when
 * the queue changes and whenever the playing track does (`trackKey`).
 */
export function usePlayingPlaylist(trackKey: unknown): number | null {
  const [playlist, setPlaylist] = useState<number | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: trackKey is the refetch trigger
  useEffect(() => {
    let alive = true;
    const load = () =>
      void getQueue().then((q) => {
        if (alive) setPlaylist(q.playlist);
      });
    load();
    let off: (() => void) | undefined;
    onQueueChanged(load).then((f) => {
      if (alive) off = f;
      else f();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, [trackKey]);
  return playlist;
}
