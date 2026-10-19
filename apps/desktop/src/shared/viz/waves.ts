import type { VizFrame } from "../ipc/viz";
import { WAVES, type WavesConfig, type WavesPalette } from "./palette";

/**
 * «Волны» (T4.2, FR-VIZ-003/010, `06` §7.1–7.2): a WebGL2 fragment shader — domain warping with
 * fBm noise in two passes (Inigo Quilez, «Domain warping», 2002 — the classic «liquid» field the
 * Apple Music / AMLL background is made of, `01a` §B.3), a 4-colour palette, vignette and grain.
 * What moves it is copied from Яндекс «Моя волна» (`01a` §B.3): in time with the beat grid, high
 * notes → glints, like/click → an impulse, the cursor → white impulses; minimal variant — two
 * colours, no glints.
 *
 * Photosensitivity (`CLAUDE.md`, WCAG 2.3.1): the beat moves geometry (warp), never brightness;
 * glints are small in area and smoothed; the one brightness lift (like/click) happens at most
 * once in `pulse.min_gap_ms`. Frames are drawn on `requestAnimationFrame` from a jitter buffer
 * (`06` §7.6), never per message; the FPS cap and render scale come from the quality level.
 */

export type WavesQuality = { fps: number; scale: number; minimal: boolean };

const VERT = `#version 300 es
in vec2 a_pos;
void main() { gl_Position = vec4(a_pos, 0.0, 1.0); }`;

const FRAG = `#version 300 es
precision highp float;
uniform vec2 u_res;
uniform float u_time;
uniform float u_warp;
uniform float u_sparkle;
uniform float u_density;
uniform float u_lift;
uniform float u_grain;
uniform vec3 u_c[4];
uniform vec4 u_ptr[8];
out vec4 o;

float hash(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}
float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), u.x),
             mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), u.x), u.y);
}
float fbm(vec2 p) {
  float v = 0.0, a = 0.5;
  mat2 m = mat2(1.6, 1.2, -1.2, 1.6);
  for (int i = 0; i < 5; i++) { v += a * noise(p); p = m * p; a *= 0.5; }
  return v;
}
void main() {
  vec2 uv = gl_FragCoord.xy / u_res;
  vec2 p = (gl_FragCoord.xy - 0.5 * u_res) / min(u_res.x, u_res.y) * 1.8;
  float t = u_time;
  // Two warping passes: q, then r over q.
  vec2 q = vec2(fbm(p + vec2(0.0, 0.0) + 0.12 * t), fbm(p + vec2(5.2, 1.3) - 0.10 * t));
  vec2 r = vec2(fbm(p + u_warp * q + vec2(1.7, 9.2) + 0.15 * t),
                fbm(p + u_warp * q + vec2(8.3, 2.8) - 0.126 * t));
  float f = fbm(p + u_warp * r);
  vec3 col = mix(u_c[3], u_c[0], smoothstep(0.15, 0.85, f));
  col = mix(col, u_c[1], clamp(dot(q, q) * 0.55, 0.0, 1.0) * 0.7);
  col = mix(col, u_c[2], smoothstep(0.45, 1.0, r.x) * 0.65);
  // Glints: sparse, small.
  if (u_sparkle > 0.0) {
    float s = noise(p * 36.0 + vec2(t * 1.7, -t * 1.3));
    col += vec3(smoothstep(u_density, 1.0, s) * u_sparkle);
  }
  // White impulses of the cursor and clicks.
  for (int i = 0; i < 8; i++) {
    vec4 pt = u_ptr[i];
    if (pt.z <= 0.0) continue;
    vec2 d = (uv - pt.xy) * vec2(u_res.x / u_res.y, 1.0);
    col += vec3(pt.z * exp(-dot(d, d) / (pt.w * pt.w)));
  }
  col *= 1.0 + u_lift;
  col *= mix(0.55, 1.0, smoothstep(1.2, 0.2, length(uv - 0.5) * 1.4));
  col += (hash(gl_FragCoord.xy + fract(t)) - 0.5) * u_grain;
  o = vec4(col, 1.0);
}`;

type Pointer = { x: number; y: number; born: number; strength: number };

/** Diagnostics for the smoke test: frames drawn, measured FPS, sampled pixels. */
export const wavesStats = { drawn: 0, fps: 0, samples: [] as number[][], lost: 0 };

export class WavesEngine {
  private gl: WebGL2RenderingContext | null = null;
  private prog: WebGLProgram | null = null;
  private loc: Record<string, WebGLUniformLocation | null> = {};
  private raf = 0;
  private running = false;
  private lastDraw = 0;
  private fpsT = 0;
  private fpsN = 0;
  private time = 0;
  private queue: VizFrame[] = [];
  private frame: VizFrame | null = null;
  private bands: [number, number, number] = [0, 0, 0];
  private from: WavesPalette | null = null;
  private to: WavesPalette | null = null;
  private paletteAt = 0;
  private speed = WAVES.speed[0];
  private lift = { at: -1e9, ms: 0 };
  private lastLift = -1e9;
  private pointers: Pointer[] = [];
  private lastMove = 0;
  private probe = false;
  private q: WavesQuality = { fps: 60, scale: 0.75, minimal: false };

  constructor(
    private canvas: HTMLCanvasElement,
    private opts: { reducedMotion: boolean; onLost: () => void; cfg?: WavesConfig },
  ) {}

  private get c(): WavesConfig {
    return this.opts.cfg ?? WAVES;
  }

  /** `false` — no WebGL2 here (the caller shows the gradient). */
  init(): boolean {
    const gl = this.canvas.getContext("webgl2", {
      antialias: false,
      alpha: false,
      depth: false,
      powerPreference: "low-power",
    });
    if (!gl) return false;
    this.gl = gl;
    this.canvas.addEventListener("webglcontextlost", this.onLost);
    const sh = (type: number, src: string) => {
      const s = gl.createShader(type);
      if (!s) return null;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      return gl.getShaderParameter(s, gl.COMPILE_STATUS) ? s : null;
    };
    const vs = sh(gl.VERTEX_SHADER, VERT);
    const fs = sh(gl.FRAGMENT_SHADER, FRAG);
    const prog = gl.createProgram();
    if (!vs || !fs || !prog) return false;
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return false;
    this.prog = prog;
    // biome-ignore lint/correctness/useHookAtTopLevel: WebGL's useProgram, not a React hook
    gl.useProgram(prog);
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    const pos = gl.getAttribLocation(prog, "a_pos");
    gl.enableVertexAttribArray(pos);
    gl.vertexAttribPointer(pos, 2, gl.FLOAT, false, 0, 0);
    for (const n of [
      "u_res",
      "u_time",
      "u_warp",
      "u_sparkle",
      "u_density",
      "u_lift",
      "u_grain",
      "u_c",
      "u_ptr",
    ]) {
      this.loc[n] = gl.getUniformLocation(prog, n);
    }
    return true;
  }

  private u(name: string): WebGLUniformLocation | null {
    return this.loc[name] ?? null;
  }

  private onLost = (e: Event) => {
    e.preventDefault();
    wavesStats.lost += 1;
    this.pause();
    this.opts.onLost();
  };

  setQuality(q: WavesQuality): void {
    this.q = q;
    this.resize();
  }

  /** The palette and the motion speed (∝ arousal) for the current track; eased over
   * `transition_ms`. */
  setVibe(palette: WavesPalette, arousal: number | null): void {
    const now = performance.now();
    this.from = this.to ? this.current(now) : palette;
    this.to = palette;
    this.paletteAt = now;
    const a = arousal ?? 0.5;
    this.speed = this.c.speed[0] + (this.c.speed[1] - this.c.speed[0]) * a;
    if (!this.running) this.draw(now);
  }

  private current(now: number): WavesPalette {
    const to = this.to ?? [];
    const from = this.from ?? to;
    const k = Math.min(1, (now - this.paletteAt) / this.c.transition_ms);
    const e = 0.5 - 0.5 * Math.cos(Math.PI * k);
    return to.map((c, i) =>
      c.map((v, j) => (from[i]?.[j] ?? v) + (v - (from[i]?.[j] ?? v)) * e),
    ) as WavesPalette;
  }

  setAudioFrame(f: VizFrame): void {
    this.queue.push(f);
    if (this.queue.length > this.c.jitter_frames + 2) this.queue.shift();
  }

  /** A pointer impulse (`06` §7.1): moves leave faint ones, at most one per `move_gap_ms`. */
  pointer(e: { x: number; y: number; kind: "move" | "down" }): void {
    if (this.opts.reducedMotion) return;
    const now = performance.now();
    const p = this.c.pointer;
    if (e.kind === "move") {
      if (now - this.lastMove < p.move_gap_ms) return;
      this.lastMove = now;
    }
    this.pointers.push({
      x: e.x,
      y: 1 - e.y,
      born: now,
      strength: e.kind === "down" ? p.strength : p.strength * 0.4,
    });
    if (this.pointers.length > p.max) this.pointers.shift();
  }

  /** Like/click: a short brightness lift, at most once in `min_gap_ms` (flashes ≤ 1 per 10 s). */
  pulse(kind: "like" | "click"): void {
    if (this.opts.reducedMotion) return;
    const now = performance.now();
    if (now - this.lastLift < this.c.pulse.min_gap_ms) return;
    this.lastLift = now;
    this.lift = { at: now, ms: kind === "like" ? this.c.pulse.like_ms : this.c.pulse.click_ms };
  }

  /** The next draw samples a few pixels into `wavesStats.samples` (smoke). */
  sample(): void {
    this.probe = true;
    if (!this.running) this.draw(performance.now());
  }

  resume(): void {
    if (this.running || !this.gl || this.opts.reducedMotion) {
      if (this.opts.reducedMotion) this.draw(performance.now());
      return;
    }
    this.running = true;
    this.lastDraw = 0;
    const loop = (now: number) => {
      if (!this.running) return;
      this.raf = requestAnimationFrame(loop);
      // The FPS cap: skip a display frame that comes too early.
      if (this.lastDraw && now - this.lastDraw < 1000 / this.q.fps - 2) return;
      const dt = this.lastDraw ? Math.min(0.1, (now - this.lastDraw) / 1000) : 0;
      this.lastDraw = now;
      this.step(dt, now);
      this.draw(now);
    };
    this.raf = requestAnimationFrame(loop);
  }

  pause(): void {
    this.running = false;
    cancelAnimationFrame(this.raf);
  }

  resize(): void {
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(this.canvas.clientWidth * dpr * this.q.scale));
    const h = Math.max(1, Math.round(this.canvas.clientHeight * dpr * this.q.scale));
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
    if (!this.running) this.draw(performance.now());
  }

  dispose(): void {
    this.pause();
    this.canvas.removeEventListener("webglcontextlost", this.onLost);
    this.gl?.getExtension("WEBGL_lose_context")?.loseContext();
    this.gl = null;
  }

  /** Takes the frame `jitter_frames` behind the newest and smooths the bands. */
  private step(dt: number, now: number): void {
    while (this.queue.length > this.c.jitter_frames) {
      this.frame = this.queue.shift() ?? this.frame;
    }
    const f = this.frame;
    const target = f?.playing ? f.bands : [0, 0, 0];
    const s = this.c.smoothing;
    for (let i = 0; i < 3; i++) {
      const cur = this.bands[i] ?? 0;
      const want = target[i] ?? 0;
      this.bands[i] = cur + (want - cur) * (want > cur ? s.attack : s.release);
    }
    // Paused: the field keeps drifting, slower.
    this.time += dt * this.speed * (f?.playing ? 1 : 0.35) * 10;
    this.fpsN += 1;
    if (now - this.fpsT >= 1000) {
      wavesStats.fps = (this.fpsN * 1000) / (now - this.fpsT || 1);
      this.fpsT = now;
      this.fpsN = 0;
    }
  }

  private draw(now: number): void {
    const gl = this.gl;
    if (!gl || !this.prog || !this.to || gl.isContextLost()) return;
    const c = this.c;
    const reduced = this.opts.reducedMotion;
    const f = this.frame;
    const beat =
      !reduced && f?.playing && f.bpm > 0 ? Math.exp(-f.beatPhase * c.warp.beat_decay) : 0;
    const warp = c.warp.base + (reduced ? 0 : this.bands[0] * c.warp.bass + beat * c.warp.beat);
    const sparkle =
      this.q.minimal || reduced ? 0 : Math.min(c.sparkle.max, this.bands[2] * c.sparkle.treble);
    const liftK = (now - this.lift.at) / (this.lift.ms || 1);
    const lift = liftK >= 0 && liftK < 1 ? c.pulse.lift * Math.sin(Math.PI * liftK) : 0;
    const pal = this.current(now);
    const ptr = new Float32Array(32);
    this.pointers = this.pointers.filter((p) => now - p.born < c.pointer.decay_ms);
    this.pointers.forEach((p, i) => {
      const k = 1 - (now - p.born) / c.pointer.decay_ms;
      ptr.set([p.x, p.y, p.strength * k * k, c.pointer.radius], i * 4);
    });
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    gl.uniform2f(this.u("u_res"), this.canvas.width, this.canvas.height);
    gl.uniform1f(this.u("u_time"), this.time);
    gl.uniform1f(this.u("u_warp"), warp);
    gl.uniform1f(this.u("u_sparkle"), sparkle);
    gl.uniform1f(this.u("u_density"), c.sparkle.density);
    gl.uniform1f(this.u("u_lift"), lift);
    gl.uniform1f(this.u("u_grain"), c.grain);
    gl.uniform3fv(this.u("u_c"), pal.flat());
    gl.uniform4fv(this.u("u_ptr"), ptr);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    wavesStats.drawn += 1;
    if (this.probe) {
      this.probe = false;
      const px = new Uint8Array(4);
      const out: number[][] = [];
      const points: [number, number][] = [
        [0.5, 0.5],
        [0.2, 0.3],
        [0.8, 0.7],
        [0.3, 0.8],
        [0.7, 0.2],
      ];
      for (const [x, y] of points) {
        gl.readPixels(
          Math.floor(x * this.canvas.width),
          Math.floor(y * this.canvas.height),
          1,
          1,
          gl.RGBA,
          gl.UNSIGNED_BYTE,
          px,
        );
        out.push(Array.from(px.subarray(0, 3)));
      }
      wavesStats.samples = out;
    }
  }
}
