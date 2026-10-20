import { useSyncExternalStore } from "react";
import type { VideoClip } from "@/shared/ipc/bindings";

/**
 * «Видео» state shared by the background and the toolbar (T4.4): the clips and the one on
 * screen.
 */
type State = { clips: VideoClip[]; current: string };

let state: State = { clips: [], current: "" };
const subs = new Set<() => void>();

export const videoControl = {
  get: (): State => state,
  set(patch: Partial<State>): void {
    state = { ...state, ...patch };
    for (const s of subs) s();
  },
  subscribe(cb: () => void): () => void {
    subs.add(cb);
    return () => subs.delete(cb);
  },
};

export function useVideoControl(): State {
  return useSyncExternalStore(videoControl.subscribe, videoControl.get);
}

/** The vibe's mood (Яндекс's four, `01b` F-16): valence × arousal quadrants. */
export function moodOf(valence: number | null, arousal: number | null): string {
  const v = valence ?? 0.5;
  const a = arousal ?? 0.5;
  if (a >= 0.5) return v >= 0.5 ? "happy" : "energetic";
  return v >= 0.5 ? "calm" : "sad";
}

/** A clip for the mood: its own, else «any», else whatever there is; stable for a track. */
export function clipFor(clips: VideoClip[], mood: string, seed: number): VideoClip | null {
  const own = clips.filter((c) => c.mood === mood);
  const any = clips.filter((c) => c.mood === "any");
  const pool = own.length > 0 ? own : any.length > 0 ? any : clips;
  if (pool.length === 0) return null;
  return pool[Math.abs(seed) % pool.length] ?? null;
}
