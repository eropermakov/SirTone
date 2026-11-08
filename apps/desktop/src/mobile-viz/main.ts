// The phone's visualiser page (stage 8, D-148): the PC's «Волны» and MilkDrop engines unchanged,
// inside the iPhone app's WKWebView, as one script (`vite.mobile-viz.config.ts`). The app pushes
// the core's S-04 frames (base64) and the track's colours; the engines draw on
// requestAnimationFrame from their own jitter buffers, as on the PC. No strings shown here: the
// page is a picture; every word is in the app.

import type { VizColors } from "@/shared/ipc/bindings";
import { parseFrame } from "@/shared/ipc/viz";
import { MilkDropEngine, milkdropStats } from "@/shared/viz/milkdrop";
import { lookOf, WAVES } from "@/shared/viz/palette";
import { MILKDROP, type PresetDesc, pick, rank, targetOf } from "@/shared/viz/presetPick";
import { WavesEngine, wavesStats } from "@/shared/viz/waves";
import index from "../../../../assets/viz/milkdrop/index.json";

type Mode = "waves" | "milkdrop";
type Start = { mode: Mode; accent: string; reduced: boolean; level: 1 | 2 | 3 | 4 };
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
    const m = new MilkDropEngine(canvas, { onLost: () => post("failed", "milkdrop") });
    if (!(await m.init())) {
      post("failed", "milkdrop");
      return;
    }
    m.setQuality(MILKDROP.quality[String(o.level) as "1"]);
    engine = { kind: "milkdrop", milk: m };
    m.resize();
    m.resume();
    rotate = window.setInterval(() => nextPreset(m), MILKDROP.preset_s * 1000);
  } else {
    const w = new WavesEngine(canvas, {
      reducedMotion: o.reduced,
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

function frame(b64: string) {
  const bin = atob(b64);
  const buf = new ArrayBuffer(bin.length);
  const u8 = new Uint8Array(buf);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  const f = parseFrame(buf);
  if (!f) return;
  engine?.waves?.setAudioFrame(f);
  engine?.milk?.setAudioFrame(f);
}

window.addEventListener("resize", () => {
  engine?.waves?.resize();
  engine?.milk?.resize();
});

// What the app measures for the spike (D-148): FPS and the worst frame, every 2 s.
window.setInterval(() => {
  if (!engine) return;
  post(
    "stats",
    engine.kind === "waves"
      ? { mode: "waves", ...wavesStats }
      : { mode: "milkdrop", ...milkdropStats },
  );
}, 2000);

(window as unknown as Record<string, unknown>).sirtoneViz = {
  start: (o: Start) => void start(o),
  colors: (c: VizColors | null) => {
    colors = c;
    applyColors();
  },
  frame,
  pulse: () => engine?.waves?.pulse("like"),
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
post("ready", true);
