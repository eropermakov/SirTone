import { Channel, isTauri } from "@tauri-apps/api/core";
import { commands, events, type VizColors, type VizPower } from "./bindings";

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

/** Starts the feed; returns the stop function. Outside Tauri — nothing comes. */
export function startViz(onFrame: (f: VizFrame) => void): () => void {
  if (!isTauri()) return () => {};
  let alive = true;
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
    if (f) onFrame(f);
  };
  // The session number: a stop names it, so a late stop never ends a newer feed.
  const session = commands.vizStart(ch);
  (window as unknown as { __sirtoneVizStats?: typeof vizStats }).__sirtoneVizStats = vizStats;
  return () => {
    alive = false;
    void session.then((id) => commands.vizStop(id));
  };
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
