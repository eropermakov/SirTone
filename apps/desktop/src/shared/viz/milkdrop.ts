import type { VizFrame } from "../ipc/viz";
import { MILKDROP, type MilkdropConfig } from "./presetPick";

/**
 * «MilkDrop» (T4.3, FR-VIZ-004, `06` §7.3): Butterchurn 2.6.7 (MIT) fed with the visualiser
 * frames instead of WebAudio (`audioLevels`, S-04), the kept presets precompiled
 * (`assets/viz/milkdrop/presets.js`, D-95). projectM's rules (`01a` §B.2): a preset lives
 * `preset_s`, the next one blends in over `blend_s`, no hard cuts. Butterchurn and the presets
 * load on first use only (a separate chunk), so «Волны» start as before.
 */

type Visualizer = {
  loadPreset(p: unknown, blend: number): void;
  render(o: {
    audioLevels: {
      timeByteArray: Uint8Array;
      timeByteArrayL: Uint8Array;
      timeByteArrayR: Uint8Array;
    };
    elapsedTime: number;
  }): void;
  setRendererSize(w: number, h: number, o?: Record<string, number>): void;
  setInternalMeshSize(w: number, h: number): void;
};

export type MilkdropQuality = MilkdropConfig["quality"]["1"];

/** Diagnostics for the smoke test: frames drawn, FPS, the worst frame and the current preset. */
export const milkdropStats = { drawn: 0, fps: 0, worstMs: 0, preset: "", loads: 0, lost: 0 };

const SILENCE = new Uint8Array(1024).fill(128);

// The presets must run under the strict CSP (no 'unsafe-eval', D-95): violations are counted for
// the smoke test.
document.addEventListener("securitypolicyviolation", () => {
  const w = window as unknown as { __sirtoneCspErrors?: number };
  w.__sirtoneCspErrors = (w.__sirtoneCspErrors ?? 0) + 1;
});

export class MilkDropEngine {
  private viz: Visualizer | null = null;
  private presets: Record<string, unknown> = {};
  private raf = 0;
  private running = false;
  private last = 0;
  private fpsT = 0;
  private fpsN = 0;
  private queue: VizFrame[] = [];
  private frame: VizFrame | null = null;
  private q: MilkdropQuality = MILKDROP.quality["3"];
  private current = "";

  constructor(
    private canvas: HTMLCanvasElement,
    private opts: { onLost: () => void },
  ) {}

  /** Loads Butterchurn and the presets; `false` — no WebGL2 or the load failed. */
  async init(): Promise<boolean> {
    try {
      const [bc, presets] = await Promise.all([
        import("butterchurn"),
        import("../../../../../assets/viz/milkdrop/presets.js"),
      ]);
      // The UMD build lands as `default` (and the CommonJS wrapper may wrap it once more).
      const unwrap = (m: unknown) => (m as { default?: unknown }).default ?? m;
      let lib = unwrap(bc);
      if (!(lib as { createVisualizer?: unknown }).createVisualizer) lib = unwrap(lib);
      this.presets = (presets as { default: Record<string, unknown> }).default;
      const { w, h } = this.size();
      this.viz = (
        lib as { createVisualizer(c: null, el: HTMLCanvasElement, o: object): Visualizer }
      ).createVisualizer(null, this.canvas, {
        width: w,
        height: h,
        meshWidth: this.q.mesh[0],
        meshHeight: this.q.mesh[1],
        pixelRatio: 1,
        textureRatio: this.q.texture_ratio,
      });
      this.canvas.addEventListener("webglcontextlost", this.onLost);
      return true;
    } catch (e) {
      (window as unknown as { __sirtoneMilkdropError?: string }).__sirtoneMilkdropError = String(e);
      return false;
    }
  }

  private onLost = (e: Event) => {
    e.preventDefault();
    milkdropStats.lost += 1;
    this.pause();
    this.opts.onLost();
  };

  names(): string[] {
    return Object.keys(this.presets);
  }

  /** Shows a preset: blended in over `blend_s` (0 — at once, the first one). */
  show(name: string, blend = MILKDROP.blend_s): void {
    const p = this.presets[name];
    if (!this.viz || !p || name === this.current) return;
    const t0 = performance.now();
    this.viz.loadPreset(p, this.current ? blend : 0);
    // Compiling a preset blocks the main thread once (S-04): measured into worstMs.
    milkdropStats.worstMs = Math.max(milkdropStats.worstMs, performance.now() - t0);
    this.current = name;
    milkdropStats.preset = name;
    milkdropStats.loads += 1;
  }

  setQuality(q: MilkdropQuality): void {
    this.q = q;
    this.viz?.setInternalMeshSize(q.mesh[0], q.mesh[1]);
    this.resize();
  }

  setAudioFrame(f: VizFrame): void {
    this.queue.push(f);
    if (this.queue.length > 4) this.queue.shift();
  }

  /** The drawing buffer in device pixels: CSS size × the preset's pixel ratio. butterchurn
   * draws to the screen at its `width × height` (no pixel ratio), so it gets the buffer's own
   * size and ratio 1 — otherwise a ratio ≠ 1 crops the picture into a corner (owner, proto 32). */
  private size() {
    const pr = this.q.pixel_ratio;
    return {
      w: Math.max(1, Math.round(this.canvas.clientWidth * pr)),
      h: Math.max(1, Math.round(this.canvas.clientHeight * pr)),
    };
  }

  resize(): void {
    const { w, h } = this.size();
    this.canvas.width = w;
    this.canvas.height = h;
    this.viz?.setRendererSize(w, h, {
      pixelRatio: 1,
      textureRatio: this.q.texture_ratio,
      meshWidth: this.q.mesh[0],
      meshHeight: this.q.mesh[1],
    });
  }

  resume(): void {
    if (this.running || !this.viz) return;
    this.running = true;
    this.last = 0;
    const loop = (now: number) => {
      if (!this.running) return;
      this.raf = requestAnimationFrame(loop);
      if (this.last && now - this.last < 1000 / this.q.fps - 2) return;
      const dt = this.last ? Math.min(0.1, (now - this.last) / 1000) : 1 / 60;
      this.last = now;
      // The jitter buffer: two frames behind the newest (`06` §7.6).
      while (this.queue.length > 2) this.frame = this.queue.shift() ?? this.frame;
      const f = this.frame;
      const quiet = !f?.playing;
      try {
        this.viz?.render({
          audioLevels: {
            timeByteArray: quiet ? SILENCE : f.mono,
            timeByteArrayL: quiet ? SILENCE : f.left,
            timeByteArrayR: quiet ? SILENCE : f.right,
          },
          elapsedTime: dt,
        });
      } catch (e) {
        // A preset that throws: remembered for diagnostics, the next draw tries again.
        (window as unknown as { __sirtoneMilkdropError?: string }).__sirtoneMilkdropError = String(
          (e as Error)?.stack ?? e,
        ).slice(0, 500);
        return;
      }
      milkdropStats.drawn += 1;
      this.fpsN += 1;
      if (now - this.fpsT >= 1000) {
        milkdropStats.fps = (this.fpsN * 1000) / (now - this.fpsT || 1);
        this.fpsT = now;
        this.fpsN = 0;
      }
    };
    this.raf = requestAnimationFrame(loop);
  }

  pause(): void {
    this.running = false;
    cancelAnimationFrame(this.raf);
  }

  /** Frees the GL context: one engine holds one at a time. */
  dispose(): void {
    this.pause();
    this.canvas.removeEventListener("webglcontextlost", this.onLost);
    const gl = this.canvas.getContext("webgl2");
    gl?.getExtension("WEBGL_lose_context")?.loseContext();
    this.viz = null;
  }
}
