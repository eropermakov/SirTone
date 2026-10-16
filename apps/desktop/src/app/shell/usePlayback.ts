import { useCallback, useEffect, useRef, useState } from "react";
import {
  EMPTY_SNAPSHOT,
  getSnapshot,
  onPlaybackError,
  onSnapshot,
  type PlaybackError,
  type PlaybackSnapshot,
  togglePlay,
} from "@/shared/ipc/playback";

/** Elements a key event must not hijack: they handle Space themselves. */
function ownsSpace(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    ["INPUT", "TEXTAREA", "SELECT", "BUTTON"].includes(target.tagName) ||
    target.getAttribute("role") === "slider"
  );
}

/**
 * Player state from the engine (snapshots every 250 ms while playing) + the last error.
 * `receivedAt` lets the progress bar extrapolate smoothly between snapshots without re-rendering.
 */
export function usePlayback() {
  const [snap, setSnap] = useState<PlaybackSnapshot>(EMPTY_SNAPSHOT);
  const [error, setError] = useState<PlaybackError | null>(null);
  const receivedAt = useRef(performance.now());

  useEffect(() => {
    const offs: Array<() => void> = [];
    let alive = true;
    const apply = (s: PlaybackSnapshot) => {
      receivedAt.current = performance.now();
      setSnap(s);
    };
    getSnapshot().then((s) => alive && apply(s));
    onSnapshot(apply).then((off) => (alive ? offs.push(off) : off()));
    onPlaybackError(setError).then((off) => (alive ? offs.push(off) : off()));
    return () => {
      alive = false;
      for (const off of offs) off();
    };
  }, []);

  // DESIGN.md §10: Space — play/pause (unless a control owns the key).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.code !== "Space" || e.repeat || ownsSpace(e.target)) return;
      e.preventDefault();
      togglePlay();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const clearError = useCallback(() => setError(null), []);
  return { snap, receivedAt, error, clearError };
}
