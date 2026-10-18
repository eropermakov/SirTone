import { isTauri } from "@tauri-apps/api/core";
import {
  commands,
  events,
  type PlaybackError,
  type PlaybackSnapshot,
  type PlayContext,
  type QueueView,
  type RepeatMode,
} from "./bindings";

/**
 * Playback IPC (prototype 0.2). Commands and events are generated (`bindings.ts`); outside Tauri
 * (browser preview, Playwright) everything is a no-op and the player stays empty.
 */
export type { PlaybackError, PlaybackSnapshot, QueueView, RepeatMode };

export const EMPTY_SNAPSHOT: PlaybackSnapshot = {
  state: "empty",
  title: null,
  artists: null,
  trackId: null,
  inWave: false,
  waveProfile: null,
  hasNext: false,
  hasPrev: false,
  cover: null,
  repeat: "off",
  shuffle: false,
  underruns: 0,
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

export function next(): void {
  if (isTauri()) void commands.playbackNext();
}

export function prev(): void {
  if (isTauri()) void commands.playbackPrev();
}

export function setRepeat(mode: RepeatMode): void {
  if (isTauri()) void commands.playbackSetRepeat(mode);
}

export function setShuffle(on: boolean): void {
  if (isTauri()) void commands.playbackSetShuffle(on);
}

export const EMPTY_QUEUE: QueueView = {
  items: [],
  position: 0,
  repeat: "off",
  shuffle: false,
  autoplay: null,
  playlist: null,
};

export async function getQueue(): Promise<QueueView> {
  return isTauri() ? commands.queueGet() : EMPTY_QUEUE;
}

/** «Воспроизвести далее» / «В конец очереди» (FR-PLAY-006). */
export function queuePlayNext(ids: number[], context: PlayContext): void {
  if (isTauri()) void commands.queuePlayNext(ids, context);
}

export function queueAppend(ids: number[], context: PlayContext): void {
  if (isTauri()) void commands.queueAppend(ids, context);
}

export function queueRemove(index: number): void {
  if (isTauri()) void commands.queueRemove(index);
}

export function queueMove(from: number, to: number): void {
  if (isTauri()) void commands.queueMove(from, to);
}

export function queueJump(index: number): void {
  if (isTauri()) void commands.queueJump(index);
}

export async function onQueueChanged(cb: () => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  return events.queueChanged.listen(() => cb());
}
