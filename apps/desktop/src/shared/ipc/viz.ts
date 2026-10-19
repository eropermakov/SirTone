import { Channel, isTauri } from "@tauri-apps/api/core";
import { commands } from "./bindings";

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
  void commands.vizStart(ch);
  (window as unknown as { __sirtoneVizStats?: typeof vizStats }).__sirtoneVizStats = vizStats;
  return () => {
    alive = false;
    void commands.vizStop();
  };
}
