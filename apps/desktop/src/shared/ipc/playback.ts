import { isTauri } from "@tauri-apps/api/core";
import { commands, events, type PlaybackError, type PlaybackSnapshot } from "./bindings";

/**
 * Playback IPC (prototype 0.2). Commands and events are generated (`bindings.ts`); outside Tauri
 * (browser preview, Playwright) everything is a no-op and the player stays empty.
 */
export type { PlaybackError, PlaybackSnapshot };

export const EMPTY_SNAPSHOT: PlaybackSnapshot = {
  state: "empty",
  title: null,
  positionMs: 0,
  durationMs: null,
  volume: 1,
};

export async function getSnapshot(): Promise<PlaybackSnapshot> {
  return isTauri() ? commands.playbackSnapshot() : EMPTY_SNAPSHOT;
}

export function togglePlay(): void {
  if (isTauri()) void commands.playbackToggle();
}

export function seek(positionMs: number): void {
  if (isTauri()) void commands.playbackSeek(Math.max(0, Math.round(positionMs)));
}

export function setVolume(volume: number): void {
  if (isTauri()) void commands.playbackSetVolume(Math.min(1, Math.max(0, volume)));
}

export async function onSnapshot(cb: (s: PlaybackSnapshot) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  return events.playbackSnapshot.listen((e) => cb(e.payload));
}

export async function onPlaybackError(cb: (e: PlaybackError) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  return events.playbackError.listen((e) => cb(e.payload));
}
