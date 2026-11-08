// The phone's visualiser page (stage 8, D-148): the PC's «Волны» and MilkDrop engines unchanged,
// inside the iPhone app's WKWebView, as one script (`vite.mobile-viz.config.ts`). The app pushes
// the core's S-04 frames (base64) and the track's colours; the engines draw on
// requestAnimationFrame from their own jitter buffers, as on the PC. No strings shown here: the
// page is a picture; every word is in the app.

import type { VizColors } from "@/shared/ipc/bindings";
import { parseFrame, type VizFrame } from "@/shared/ipc/viz";
import { MilkDropEngine, milkdropStats } from "@/shared/viz/milkdrop";
import { lookOf, WAVES } from "@/shared/viz/palette";
import { MILKDROP, type PresetDesc, pick, rank, targetOf } from "@/shared/viz/presetPick";
import { WavesEngine, wavesMood, wavesStats } from "@/shared/viz/waves";
import index from "../../../../assets/viz/milkdrop/index.json";

type Mode = "waves" | "milkdrop";
type Start = {
  mode: Mode;
  accent: string;
  reduced: boolean;
  level: 1 | 2 | 3 | 4;
  /** `full` — the full-screen visualiser, `bg` — behind the player (the numbers are kept apart). */
  role?: string;
  lowPower?: boolean;
};
type Engine = { kind: Mode; waves?: WavesEngine; milk?: MilkDropEngine };

const RANKED = rank((index as { presets: PresetDesc[] }).presets);
/** A fresh canvas per engine: a disposed engine leaves its GL context lost on the old one. */
function freshCanvas(): HTMLCanvasElement {
  const old = document.getElementById("viz");
  const c = document.createElement("canvas");
  c.id = "viz";
  if (old) old.replaceWith(c);
  else document.body.appendChild(c);
  return c;
}
const post = (name: string, body: unknown) =>
  (
    window as unknown as {
      webkit?: { messageHandlers?: Record<string, { postMessage(b: unknown): void }> };
    }
  ).webkit?.messageHandlers?.[name]?.postMessage(body);

let opts: Start = { mode: "waves", accent: "#7ab800", reduced: false, level: 3 };
let colors: VizColors | null = null;
let engine: Engine | null = null;
let rotate = 0;
const recent: string[] = [];

function nextPreset(e: MilkDropEngine) {
  for (let attempt = 0; attempt < 5; attempt++) {
    const name = pick(RANKED, targetOf(colors), { recent, ratings: new Map(), onlyLiked: false });
    if (!name) return;
    recent.push(name);
    if (recent.length > MILKDROP.recent) recent.shift();
    try {
      e.show(name);
      return;
    } catch {
      // A preset that fails to compile is skipped, as on the PC.
    }
  }
}

function applyColors() {
  const e = engine;
  if (!e) return;
  if (e.waves) {
    const minimal = WAVES.quality[String(opts.level) as "1"].minimal;
    e.waves.setVibe(
      lookOf({ colors, accent: opts.accent, followAccent: false, minimal, themeViz: null }),
    );
  }
  if (e.milk) nextPreset(e.milk);
}

function stop() {
  window.clearInterval(rotate);
  engine?.waves?.dispose();
  engine?.milk?.dispose();
  engine = null;
}

async function start(o: Start) {
  stop();
  opts = o;
  const canvas = freshCanvas();
  if (o.mode === "milkdrop") {
    const m = new MilkDropEngine(canvas, { onLost: () => post("failed", "milkdrop: lost") });
    if (!(await m.init())) {
      const why = (window as unknown as { __sirtoneMilkdropError?: string }).__sirtoneMilkdropError;
      post("failed", `milkdrop: ${why ?? "init"}`);
      return;
    }
    m.setQuality(MILKDROP.quality[String(o.level) as "1"]);
    engine = { kind: "milkdrop", milk: m };
    m.resize();
    m.resume();
    rotate = window.setInterval(() => nextPreset(m), MILKDROP.preset_s * 1000);
  } else {
    // «Уменьшение движения» (owner, 2026-11-08): the visualiser still flows — at half speed,
    // with no pulses or flashes (the page never calls `pulse` then) — instead of one still frame.
    wavesMood.speed = o.reduced ? 0.5 : 1;
    const w = new WavesEngine(canvas, {
      reducedMotion: false,
      onLost: () => post("failed", "waves"),
    });
    if (!w.init()) {
      post("failed", "waves");
      return;
    }
    w.setQuality(WAVES.quality[String(o.level) as "1"]);
    engine = { kind: "waves", waves: w };
    w.resize();
    w.resume();
  }
  applyColors();
}

/** Frames received per second (the app's push) and the page's own display rate (rAF). */
const rate = { fed: 0, fedN: 0, raf: 0, rafN: 0, at: performance.now(), handlerMs: 0 };
function countRaf() {
  rate.rafN += 1;
  requestAnimationFrame(countRaf);
}
requestAnimationFrame(countRaf);

/** «Волны» need only the header (bands, beat): the app sends seven numbers, not 4 KB of samples. */
const SILENT = new Uint8Array(1024).fill(128);
function bands(
  playing: boolean,
  t: number,
  phase: number,
  bpm: number,
  b0: number,
  b1: number,
  b2: number,
) {
  const t0 = performance.now();
  rate.fedN += 1;
  const f: VizFrame = {
    playing,
    tAudioMs: t,
    beatPhase: phase,
    bpm,
    bands: [b0, b1, b2],
    mono: SILENT,
    left: SILENT,
    right: SILENT,
  };
  engine?.waves?.setAudioFrame(f);
  engine?.milk?.setAudioFrame(f);
  rate.handlerMs = Math.max(rate.handlerMs * 0.99, performance.now() - t0);
}

function frame(b64: string) {
  const t0 = performance.now();
  rate.fedN += 1;
  const bin = atob(b64);
  const buf = new ArrayBuffer(bin.length);
  const u8 = new Uint8Array(buf);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  const f = parseFrame(buf);
  if (!f) return;
  engine?.waves?.setAudioFrame(f);
  engine?.milk?.setAudioFrame(f);
  rate.handlerMs = Math.max(rate.handlerMs * 0.99, performance.now() - t0);
}

window.addEventListener("resize", () => {
  engine?.waves?.resize();
  engine?.milk?.resize();
});

/** Seconds in a row the engine draws well below what the display gives it: after 4 the page steps
 * one quality level down by itself (D-148). Measured against the display's own rate — in Low Power
 * Mode iOS gives 30 a second, which is no reason to drop. */
let slow = 0;

function adapt() {
  if (!engine || opts.level <= 1) return;
  const fps = engine.kind === "waves" ? wavesStats.fps : milkdropStats.fps;
  const cap =
    engine.kind === "waves" ? WAVES.quality[String(opts.level) as "1"].fps : rate.raf || 60;
  const want = Math.min(rate.raf || 60, cap) * 0.75;
  slow = fps > 0 && fps < want ? slow + 2 : 0;
  if (slow < 4) return;
  slow = 0;
  opts = { ...opts, level: (opts.level - 1) as Start["level"] };
  engine.waves?.setQuality(WAVES.quality[String(opts.level) as "1"]);
  engine.milk?.setQuality(MILKDROP.quality[String(opts.level) as "1"]);
}

// What the app measures for the spike (D-148): FPS, the worst frame and the level, every 2 s.
window.setInterval(() => {
  if (!engine) return;
  const now = performance.now();
  const span = (now - rate.at) / 1000 || 1;
  rate.fed = rate.fedN / span;
  rate.raf = rate.rafN / span;
  rate.fedN = 0;
  rate.rafN = 0;
  rate.at = now;
  adapt();
  const c = document.getElementById("viz") as HTMLCanvasElement | null;
  const common = {
    role: opts.role ?? "full",
    level: opts.level,
    lowPower: !!opts.lowPower,
    reduced: opts.reduced,
    rafHz: Math.round(rate.raf),
    fedHz: Math.round(rate.fed),
    handlerMs: Math.round(rate.handlerMs * 100) / 100,
    canvas: c ? `${c.width}x${c.height}` : "",
    dpr: window.devicePixelRatio,
  };
  post(
    "stats",
    engine.kind === "waves"
      ? { mode: "waves", ...common, fps: wavesStats.fps, drawn: wavesStats.drawn }
      : { mode: "milkdrop", ...common, ...milkdropStats },
  );
}, 2000);

(window as unknown as Record<string, unknown>).sirtoneViz = {
  start: (o: Start) => void start(o),
  colors: (c: VizColors | null) => {
    colors = c;
    applyColors();
  },
  frame,
  bands,
  pulse: () => {
    if (!opts.reduced) engine?.waves?.pulse("like");
  },
  pause: () => {
    engine?.waves?.pause();
    engine?.milk?.pause();
  },
  resume: () => {
    engine?.waves?.resume();
    engine?.milk?.resume();
  },
  stop,
};
// Anything that breaks on the phone reaches the app (saved with the numbers for the PC to read).
window.addEventListener("error", (e) => post("failed", `error: ${e.message}`));
window.addEventListener("unhandledrejection", (e) =>
  post("failed", `rejection: ${String(e.reason)}`),
);
post("ready", true);
