import { latestVizFrame } from "../ipc/viz";
import { FlashClock, FX, type FxConfig, type FxPlan, fxColors } from "./fxPlan";

/**
 * Live effects (T4.20, FR-FX-004…007, `06` §7.7, `14` §13.2, spike S-19 / D-46): its own WebGL2
 * canvas over the visualiser and under the scrim, `pointer-events: none`. Particles cost no CPU
 * per frame: one static buffer of seeds for the largest budget, the motion is a function of
 * `(seed, t, wind, breath)` in the vertex shader, one `drawArraysInstanced` call; the density is
 * the instance count. Fog and cloud shadows are an fBm noise pass drawn at ¼ resolution (the
 * browser's upscale blurs it for free). Music gives only the «breath»: a small swirl on the beat
 * and a brightness pulse from the bass — never the count. The storm's flashes come from
 * `FlashClock` (≤ 1 in 10 s, smooth).
 */

export const fxStats = { drawn: 0, fps: 0, count: 0, type: "", lost: 0, still: 0, error: "" };

const MAX = 5000;
const SHAPES = { dot: 0, streak: 1, leaf: 2, glow: 3, noise: 0 } as const;

const PVERT = `#version 300 es
layout(location = 0) in vec2 a_corner;
layout(location = 1) in vec4 a_seed;
uniform vec2 u_res;
uniform float u_px;
uniform float u_time;
uniform vec2 u_size;
uniform vec2 u_len;
uniform vec2 u_fall;
uniform float u_sway;
uniform vec2 u_swayHz;
uniform float u_spin;
uniform float u_wind;
uniform float u_planes;
uniform float u_swirl;
out vec2 v_uv;
out float v_depth;
out float v_pick;
out float v_phase;
void main() {
  float depth = u_planes > 1.5 ? floor(a_seed.w * u_planes) / (u_planes - 1.0) : 1.0;
  float sc = mix(0.45, 1.0, depth);
  float r3 = fract(a_seed.z * 7.13 + a_seed.x * 3.7);
  float speed = mix(u_fall.x, u_fall.y, r3) * sc;
  float y = fract(a_seed.y + u_time * speed);
  float hz = mix(u_swayHz.x, u_swayHz.y, a_seed.z);
  float sway = sin(u_time * hz * 6.2832 + a_seed.w * 31.0) * u_sway * sc;
  float drift = u_wind * u_time * abs(speed) * 0.6;
  float swirl = u_swirl * sin(y * 12.566 + a_seed.z * 6.2832);
  float x = fract(a_seed.x + sway + drift + swirl);
  vec2 center = vec2(x * 1.08 - 0.04, 1.04 - y * 1.08);
  float size = mix(u_size.x, u_size.y, a_seed.z) * sc * u_px;
  vec2 halfSz = vec2(size) * 0.5;
  float ang = u_spin * u_time * (a_seed.w - 0.5) * 2.0 + a_seed.x * 6.2832;
  float squash = 1.0;
  if (u_len.y > 0.0) {
    halfSz.y = mix(u_len.x, u_len.y, r3) * sc * u_px * 0.5;
    ang = atan(u_wind * 0.6, 1.0);
  } else if (u_spin > 0.0) {
    squash = max(0.2, abs(cos(u_time * (0.6 + hz * 4.0) + a_seed.y * 20.0)));
  }
  vec2 q = a_corner * halfSz * vec2(squash, 1.0);
  float cs = cos(ang);
  float sn = sin(ang);
  q = vec2(cs * q.x - sn * q.y, sn * q.x + cs * q.y);
  vec2 p = center * u_res + q;
  gl_Position = vec4(p / u_res * 2.0 - 1.0, 0.0, 1.0);
  v_uv = a_corner;
  v_depth = depth;
  v_pick = a_seed.y;
  v_phase = a_seed.z * 6.2832 + a_seed.x * 13.0;
}`;

const PFRAG = `#version 300 es
precision highp float;
in vec2 v_uv;
in float v_depth;
in float v_pick;
in float v_phase;
uniform int u_shape;
uniform vec3 u_c[3];
uniform int u_nc;
uniform float u_alpha;
uniform float u_soft;
uniform float u_time;
uniform float u_blink;
uniform float u_bright;
out vec4 o;
void main() {
  float r = length(v_uv);
  float soft = clamp(u_soft + (1.0 - v_depth) * 0.3, 0.05, 0.95);
  float a;
  if (u_shape == 0) {
    a = 1.0 - smoothstep(1.0 - soft, 1.0, r);
  } else if (u_shape == 1) {
    a = (1.0 - smoothstep(0.2, 1.0, abs(v_uv.x))) * (1.0 - smoothstep(0.1, 1.0, abs(v_uv.y)));
  } else if (u_shape == 2) {
    float e = length(v_uv * vec2(1.0, 0.62));
    a = 1.0 - smoothstep(0.9 - soft * 0.5, 0.98, e);
    a *= 1.0 - 0.35 * (1.0 - smoothstep(0.0, 0.08, abs(v_uv.x)));
  } else {
    a = exp(-r * r * 4.0);
  }
  int i = clamp(int(floor(v_pick * float(u_nc))), 0, 2);
  vec3 col = u_c[i];
  float blink = u_blink > 0.5 ? 0.15 + 0.85 * max(0.0, sin(u_time * 0.9 + v_phase)) : 1.0;
  float alpha = a * u_alpha * blink * mix(0.55, 1.0, v_depth) * u_bright;
  o = vec4(col * alpha, alpha);
}`;

const NVERT = `#version 300 es
layout(location = 0) in vec2 a_pos;
void main() { gl_Position = vec4(a_pos, 0.0, 1.0); }`;

const NFRAG = `#version 300 es
precision highp float;
uniform vec2 u_res;
uniform float u_time;
uniform float u_scale;
uniform float u_wind;
uniform float u_alpha;
uniform float u_shadow;
uniform vec3 u_col;
uniform float u_bright;
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
  for (int k = 0; k < 4; k++) { v += a * noise(p); p = m * p; a *= 0.5; }
  return v;
}
void main() {
  vec2 p = gl_FragCoord.xy / u_res.y * u_scale;
  p += vec2(u_wind * u_time, 0.12 * u_time);
  float n = fbm(p + 0.6 * fbm(p * 0.7 - 0.05 * u_time));
  float a = smoothstep(0.42, 0.85, n) * u_alpha * u_bright;
  vec3 col = u_shadow > 0.5 ? u_col * 0.18 : u_col;
  o = vec4(col * a, a);
}`;

type Uniforms = Record<string, WebGLUniformLocation | null>;

/** A small deterministic generator: a still frame looks the same every time. */
function mulberry32(seed: number): () => number {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export class FxEngine {
  private gl: WebGL2RenderingContext | null = null;
  private part: WebGLProgram | null = null;
  private noise: WebGLProgram | null = null;
  private pu: Uniforms = {};
  private nu: Uniforms = {};
  private vaoP: WebGLVertexArrayObject | null = null;
  private vaoN: WebGLVertexArrayObject | null = null;
  private plan: FxPlan | null = null;
  private next: FxPlan | null | undefined;
  private fadeAt = 0;
  private fade = 1;
  private running = false;
  private raf = 0;
  private lastDraw = 0;
  private time = 0;
  private bass = 0;
  /** The beat's envelope, glided (the swirl never jumps, D-107). */
  private swirl = 0;
  private breathAt = 0;
  private flash: FlashClock | null = null;
  private fpsN = 0;
  private fpsT = 0;
  /** The plan's colours, packed once per plan (nothing allocated per frame). */
  private colors = new Float32Array(9);
  private nColors = 1;

  constructor(
    private canvas: HTMLCanvasElement,
    private opts: { reducedMotion: boolean; onLost: () => void },
    private c: FxConfig = FX,
  ) {}

  /** `false` — no WebGL2: there are simply no effects. */
  init(): boolean {
    const gl = this.canvas.getContext("webgl2", {
      antialias: false,
      alpha: true,
      premultipliedAlpha: true,
      depth: false,
      powerPreference: "low-power",
    });
    if (!gl) return false;
    this.gl = gl;
    this.canvas.addEventListener("webglcontextlost", this.onLost);
    const program = (vsSrc: string, fsSrc: string) => {
      const sh = (type: number, src: string) => {
        const s = gl.createShader(type);
        if (!s) return null;
        gl.shaderSource(s, src);
        gl.compileShader(s);
        if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) {
          fxStats.error = gl.getShaderInfoLog(s) ?? "compile";
          return null;
        }
        return s;
      };
      const vs = sh(gl.VERTEX_SHADER, vsSrc);
      const fs = sh(gl.FRAGMENT_SHADER, fsSrc);
      const p = gl.createProgram();
      if (!vs || !fs || !p) return null;
      gl.attachShader(p, vs);
      gl.attachShader(p, fs);
      gl.linkProgram(p);
      if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
        fxStats.error = gl.getProgramInfoLog(p) ?? "link";
        return null;
      }
      return p;
    };
    this.part = program(PVERT, PFRAG);
    this.noise = program(NVERT, NFRAG);
    if (!this.part || !this.noise) return false;
    for (const n of [
      "u_res",
      "u_px",
      "u_time",
      "u_size",
      "u_len",
      "u_fall",
      "u_sway",
      "u_swayHz",
      "u_spin",
      "u_wind",
      "u_planes",
      "u_swirl",
      "u_shape",
      "u_c",
      "u_nc",
      "u_alpha",
      "u_soft",
      "u_blink",
      "u_bright",
    ]) {
      this.pu[n] = gl.getUniformLocation(this.part, n);
    }
    for (const n of [
      "u_res",
      "u_time",
      "u_scale",
      "u_wind",
      "u_alpha",
      "u_shadow",
      "u_col",
      "u_bright",
    ]) {
      this.nu[n] = gl.getUniformLocation(this.noise, n);
    }
    // Particles: a unit quad and the seeds of every possible instance, uploaded once.
    this.vaoP = gl.createVertexArray();
    gl.bindVertexArray(this.vaoP);
    const quad = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, quad);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    const rnd = mulberry32(0x5eed);
    const seeds = new Float32Array(MAX * 4);
    for (let i = 0; i < seeds.length; i++) seeds[i] = rnd();
    const sb = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, sb);
    gl.bufferData(gl.ARRAY_BUFFER, seeds, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 4, gl.FLOAT, false, 0, 0);
    gl.vertexAttribDivisor(1, 1);
    // Noise: one full-screen triangle.
    this.vaoN = gl.createVertexArray();
    gl.bindVertexArray(this.vaoN);
    const tri = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, tri);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    gl.bindVertexArray(null);
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    return true;
  }

  private onLost = (e: Event) => {
    e.preventDefault();
    fxStats.lost += 1;
    this.pause();
    this.opts.onLost();
  };

  /** The effect to show; a different type cross-fades (out, then in) within `transition_ms`. */
  setPlan(plan: FxPlan | null): void {
    const same =
      plan &&
      this.plan &&
      plan.type === this.plan.type &&
      plan.count === this.plan.count &&
      plan.planes === this.plan.planes &&
      plan.still === this.plan.still;
    if (same) {
      this.plan = plan;
      return;
    }
    if (!this.plan || this.opts.reducedMotion || plan?.still || !this.running) {
      this.plan = plan;
      this.next = undefined;
      this.fade = 1;
      this.applyPlan();
      return;
    }
    this.next = plan;
    this.fadeAt = performance.now();
  }

  private applyPlan(): void {
    const p = this.plan;
    fxStats.type = p?.type ?? "";
    fxStats.count = p?.count ?? 0;
    this.flash = p?.flash ? new FlashClock(performance.now()) : null;
    if (p) {
      const cs = fxColors(p.type, this.c).slice(0, 3);
      this.colors.fill(1);
      for (const [i, c] of cs.entries()) this.colors.set(c, i * 3);
      this.nColors = Math.max(1, cs.length);
    }
    this.resize();
    if (p?.still) this.drawStill();
    else if (p && !this.running) this.resume();
    else if (!p) this.pause();
  }

  resume(): void {
    if (this.running || !this.gl || !this.plan || this.plan.still) return;
    this.running = true;
    this.lastDraw = 0;
    const loop = (now: number) => {
      if (!this.running) return;
      this.raf = requestAnimationFrame(loop);
      const fps = this.plan?.fps || 60;
      if (this.lastDraw && now - this.lastDraw < 1000 / fps - 2) return;
      const dt = this.lastDraw ? Math.min(0.1, (now - this.lastDraw) / 1000) : 0;
      this.lastDraw = now;
      this.time += dt;
      this.step(now);
      this.draw(now);
    };
    this.raf = requestAnimationFrame(loop);
  }

  pause(): void {
    this.running = false;
    cancelAnimationFrame(this.raf);
  }

  resize(): void {
    const noise = this.plan && this.c.types[this.plan.type].shape === "noise";
    const scale = noise ? this.c.noise_scale : 1;
    const dpr = window.devicePixelRatio || 1;
    const w = Math.max(1, Math.round(this.canvas.clientWidth * dpr * scale));
    const h = Math.max(1, Math.round(this.canvas.clientHeight * dpr * scale));
    if (this.canvas.width !== w || this.canvas.height !== h) {
      this.canvas.width = w;
      this.canvas.height = h;
    }
    if (this.plan?.still) this.drawStill();
  }

  dispose(): void {
    this.pause();
    this.canvas.removeEventListener("webglcontextlost", this.onLost);
    this.gl?.getExtension("WEBGL_lose_context")?.loseContext();
    this.gl = null;
  }

  /**
   * Diagnostics for spike S-19 (not used by the app): the GPU time of the effects' own draws
   * over the next `n` frames, median in ms, by `EXT_disjoint_timer_query_webgl2`; without the
   * extension — the median wall time of a draw waited for with `gl.finish()` (an upper bound).
   */
  async probeGpu(n = 60): Promise<{ ms: number; method: string }> {
    const gl = this.gl;
    if (!gl) return { ms: -1, method: "none" };
    const ext = gl.getExtension("EXT_disjoint_timer_query_webgl2") as {
      TIME_ELAPSED_EXT: number;
      GPU_DISJOINT_EXT: number;
    } | null;
    const ms: number[] = [];
    if (!ext) {
      for (let i = 0; i < n; i++) {
        const t0 = performance.now();
        this.draw(t0);
        gl.finish();
        ms.push(performance.now() - t0);
      }
    } else {
      const pending: WebGLQuery[] = [];
      for (let i = 0; i < n; i++) {
        await new Promise(requestAnimationFrame);
        const q = gl.createQuery();
        if (!q) break;
        gl.beginQuery(ext.TIME_ELAPSED_EXT, q);
        this.draw(performance.now());
        gl.endQuery(ext.TIME_ELAPSED_EXT);
        pending.push(q);
      }
      for (let tries = 0; tries < 120 && pending.length > 0; tries++) {
        await new Promise(requestAnimationFrame);
        if (gl.getParameter(ext.GPU_DISJOINT_EXT)) {
          for (const q of pending.splice(0)) gl.deleteQuery(q);
          break;
        }
        while (pending.length > 0) {
          const q = pending[0] as WebGLQuery;
          if (!gl.getQueryParameter(q, gl.QUERY_RESULT_AVAILABLE)) break;
          ms.push(gl.getQueryParameter(q, gl.QUERY_RESULT) / 1e6);
          gl.deleteQuery(q);
          pending.shift();
        }
      }
    }
    ms.sort((a, b) => a - b);
    return {
      ms: ms[Math.floor(ms.length / 2)] ?? -1,
      method: ext ? `timer query (${ms.length})` : "gl.finish",
    };
  }

  /** One frame at a fixed moment, then nothing (level 1, reduced motion). */
  drawStill(): void {
    this.pause();
    this.time = 37;
    this.bass = 0;
    this.draw(performance.now(), true);
    fxStats.still += 1;
  }

  /** The type change: fade out, swap, fade in (≤ `transition_ms`). */
  private step(now: number): void {
    if (this.next !== undefined) {
      const half = this.c.transition_ms / 2;
      const k = (now - this.fadeAt) / half;
      if (k < 1) this.fade = 1 - k;
      else if (this.plan !== this.next) {
        this.plan = this.next;
        this.applyPlan();
        this.fade = 0;
      } else if (k < 2) this.fade = k - 1;
      else {
        this.fade = 1;
        this.next = undefined;
      }
    }
    // The breath (D-107): the bass and the beat's envelope glide both ways — the particles
    // sway and brighten softly, nothing jumps on a beat.
    const f = latestVizFrame();
    const dt = this.breathAt ? Math.min(0.1, (now - this.breathAt) / 1000) : 0;
    this.breathAt = now;
    const b = this.c.breath;
    const glide = (cur: number, want: number) =>
      dt > 0
        ? cur + (want - cur) * (1 - Math.exp(-dt / (want > cur ? b.attack_s : b.release_s)))
        : cur;
    const live = !this.opts.reducedMotion && f?.playing;
    this.bass = glide(this.bass, live ? (f?.bands[0] ?? 0) : 0);
    const beat = live && (f?.bpm ?? 0) > 0 ? Math.exp(-(f?.beatPhase ?? 0) * b.beat_decay) : 0;
    this.swirl = glide(this.swirl, beat);
    this.fpsN += 1;
    if (now - this.fpsT >= 1000) {
      fxStats.fps = (this.fpsN * 1000) / (now - this.fpsT || 1);
      this.fpsT = now;
      this.fpsN = 0;
    }
  }

  private draw(now: number, still = false): void {
    const gl = this.gl;
    const plan = this.plan;
    if (!gl || gl.isContextLost()) return;
    gl.viewport(0, 0, this.canvas.width, this.canvas.height);
    const flash = this.flash && !still ? this.flash.level(now) : 0;
    gl.clearColor(flash, flash, flash, flash);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (!plan) return;
    const t = this.c.types[plan.type];
    const bright = (1 + this.c.breath.bass_alpha * this.bass) * this.fade;
    if (t.shape === "noise") {
      if (!this.noise || !this.vaoN) return;
      // biome-ignore lint/correctness/useHookAtTopLevel: WebGL's useProgram, not a React hook
      gl.useProgram(this.noise);
      gl.bindVertexArray(this.vaoN);
      const u = this.nu;
      gl.uniform2f(u.u_res ?? null, this.canvas.width, this.canvas.height);
      gl.uniform1f(u.u_time ?? null, this.time * (t.speed ?? 0.02) * 10);
      gl.uniform1f(u.u_scale ?? null, t.scale ?? 2);
      gl.uniform1f(u.u_wind ?? null, t.wind * 0.1);
      gl.uniform1f(u.u_alpha ?? null, t.alpha);
      gl.uniform1f(u.u_shadow ?? null, t.shadow ? 1 : 0);
      gl.uniform3fv(u.u_col ?? null, this.colors.subarray(0, 3));
      gl.uniform1f(u.u_bright ?? null, bright);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    } else {
      if (!this.part || !this.vaoP) return;
      // biome-ignore lint/correctness/useHookAtTopLevel: WebGL's useProgram, not a React hook
      gl.useProgram(this.part);
      gl.bindVertexArray(this.vaoP);
      const u = this.pu;
      const shape = SHAPES[t.shape];
      gl.uniform2f(u.u_res ?? null, this.canvas.width, this.canvas.height);
      gl.uniform1f(u.u_px ?? null, this.canvas.height / 1080);
      gl.uniform1f(u.u_time ?? null, this.time);
      gl.uniform2fv(u.u_size ?? null, t.size ?? [3, 6]);
      gl.uniform2fv(u.u_len ?? null, t.length ?? [0, 0]);
      gl.uniform2fv(u.u_fall ?? null, t.fall ?? [0.05, 0.1]);
      gl.uniform1f(u.u_sway ?? null, t.sway ?? 0);
      gl.uniform2fv(u.u_swayHz ?? null, t.sway_hz ?? [0, 0]);
      gl.uniform1f(u.u_spin ?? null, t.spin ?? 0);
      gl.uniform1f(u.u_wind ?? null, t.wind);
      gl.uniform1f(u.u_planes ?? null, plan.planes);
      gl.uniform1f(u.u_swirl ?? null, still ? 0 : this.c.breath.swirl * this.swirl);
      gl.uniform1i(u.u_shape ?? null, shape);
      gl.uniform3fv(u.u_c ?? null, this.colors);
      gl.uniform1i(u.u_nc ?? null, this.nColors);
      gl.uniform1f(u.u_alpha ?? null, t.alpha);
      gl.uniform1f(u.u_soft ?? null, t.soft ?? 0.4);
      gl.uniform1f(u.u_blink ?? null, t.blink ? 1 : 0);
      gl.uniform1f(u.u_bright ?? null, bright);
      gl.drawArraysInstanced(gl.TRIANGLE_STRIP, 0, 4, Math.min(MAX, plan.count));
    }
    gl.bindVertexArray(null);
    fxStats.drawn += 1;
  }
}
