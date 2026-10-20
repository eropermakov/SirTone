import { Channel, isTauri } from "@tauri-apps/api/core";
import {
  commands,
  events,
  type PresetRating,
  type VideoClip,
  type VideosAdded,
  type VizColors,
  type VizPower,
} from "./bindings";

/**
 * The visualiser feed (T4.1, `04` §9, D-14): 60 frames a second of what is being heard, as raw
 * bytes — a 32-byte header (version, flags, t_audio_ms, beat_phase, bpm, bands bass/mid/treb)
 * and `u8[1024]` mono, L, R with offset 128 (Butterchurn's `timeByteArray*`). The arrays are
 * views into the received buffer — no copies.
 */
export type VizFrame = {
  playing: boolean;
  tAudioMs: number;
  beatPhase: number;
  bpm: number;
  /** bass, mid, treble 0…1. */
  bands: [number, number, number];
  mono: Uint8Array;
  left: Uint8Array;
  right: Uint8Array;
};

const WINDOW = 1024;
const FRAME = 32 + 3 * WINDOW;

/** Frame arrival statistics (smoke / diagnostics): count, the last gap and the worst one. */
export const vizStats = { frames: 0, lastGapMs: 0, maxGapMs: 0 };

export function parseFrame(buf: ArrayBuffer): VizFrame | null {
  if (buf.byteLength < FRAME) return null;
  const v = new DataView(buf);
  if (v.getUint8(0) !== 1) return null;
  return {
    playing: (v.getUint8(1) & 1) !== 0,
    tAudioMs: Number(v.getBigUint64(2, true)),
    beatPhase: v.getFloat32(10, true),
    bpm: v.getFloat32(14, true),
    bands: [v.getFloat32(18, true), v.getFloat32(22, true), v.getFloat32(26, true)],
    mono: new Uint8Array(buf, 32, WINDOW),
    left: new Uint8Array(buf, 32 + WINDOW, WINDOW),
    right: new Uint8Array(buf, 32 + 2 * WINDOW, WINDOW),
  };
}

const listeners = new Set<(f: VizFrame) => void>();
let stopFeed: (() => void) | null = null;
let latest: VizFrame | null = null;
/** The open feed's number: a closing older feed must not clear a newer one's frame. */
let feedNo = 0;

function openFeed(): () => void {
  let alive = true;
  const no = ++feedNo;
  let last = 0;
  const ch = new Channel<unknown>();
  ch.onmessage = (msg) => {
    if (!alive || !(msg instanceof ArrayBuffer)) return;
    const now = performance.now();
    if (last > 0) {
      vizStats.lastGapMs = now - last;
      vizStats.maxGapMs = Math.max(vizStats.maxGapMs, vizStats.lastGapMs);
    }
    last = now;
    vizStats.frames += 1;
    const f = parseFrame(msg);
    if (!f) return;
    latest = f;
    for (const l of listeners) l(f);
  };
  // The session number: a stop names it, so a late stop never ends a newer feed.
  const session = commands.vizStart(ch);
  (window as unknown as { __sirtoneVizStats?: typeof vizStats }).__sirtoneVizStats = vizStats;
  return () => {
    alive = false;
    if (feedNo === no) latest = null;
    void session.then((id) => commands.vizStop(id));
  };
}

/**
 * Subscribes to the feed; returns the unsubscribe. One channel serves every subscriber (the
 * visualiser and the effects layer, T4.20): it opens with the first and stops with the last.
 * Outside Tauri — nothing comes.
 */
export function startViz(onFrame: (f: VizFrame) => void): () => void {
  if (!isTauri()) return () => {};
  listeners.add(onFrame);
  if (!stopFeed) stopFeed = openFeed();
  return () => {
    if (!listeners.delete(onFrame)) return;
    if (listeners.size === 0 && stopFeed) {
      stopFeed();
      stopFeed = null;
    }
  };
}

/** The last frame heard (`null` — no feed): for readers that draw on their own clock. */
export function latestVizFrame(): VizFrame | null {
  return latest;
}

/** The colours of a track for the «Волны» palette (T4.2); `null` outside Tauri or on error. */
export async function vizColors(
  trackId: number | null,
  cover: string | null,
): Promise<VizColors | null> {
  if (!isTauri()) return null;
  const r = await commands.vizColors(trackId, cover);
  return r.status === "ok" ? r.data : null;
}

/** Pause / battery for the visualiser (T4.6): sent by the feed thread when it changes. */
export async function onVizPower(cb: (p: VizPower) => void): Promise<() => void> {
  return isTauri() ? events.vizPower.listen((e) => cb(e.payload)) : () => {};
}

/** «Ночной вайб» (T4.7): full screen and the display kept awake while playing. */
export async function nightSet(on: boolean): Promise<void> {
  if (isTauri()) await commands.nightSet(on);
}

/** MilkDrop preset ratings (T4.3): ♥ 1, ✕ −1. */
export async function presetRatings(): Promise<PresetRating[]> {
  return isTauri() ? commands.vizPresetRatings() : [];
}

export async function presetRate(name: string, rating: number): Promise<void> {
  if (isTauri()) await commands.vizPresetRate(name, rating);
}

/** «Видео» clips (T4.4): the person's own loops in the library's backgrounds folder. */
export async function videoClips(): Promise<VideoClip[]> {
  return isTauri() ? commands.vizVideos() : [];
}

export async function videoSetMood(id: string, mood: string): Promise<void> {
  if (isTauri()) await commands.vizVideoSetMood(id, mood);
}

export async function videoRemove(id: string): Promise<void> {
  if (isTauri()) await commands.vizVideoRemove(id);
}

export async function onVideosAdded(cb: (e: VideosAdded) => void): Promise<() => void> {
  return isTauri() ? events.videosAdded.listen((e) => cb(e.payload)) : () => {};
}

/** The URL `<video>` plays a clip from (HTTP ranges, `backgrounds.rs`). */
export const videoUrl = (id: string) => `http://sirtone-bg.localhost/${id}`;
