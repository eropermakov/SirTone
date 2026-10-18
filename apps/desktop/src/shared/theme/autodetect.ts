import type { PerfLevel } from "./performance";

/**
 * Performance auto-detect (T2.4, FR-PERF-003, `docs/06` §6.1): hardware facts from Rust (DXGI,
 * RAM, cores) + a 1.5 s WebGL benchmark — the median frame of a «Волны»-like shader at 1080p.
 * Thresholds, calibration v1 (owner's PC: Ryzen APU, 16 threads, 16 GB → «Мощное»):
 * median ≤ 6 ms and a discrete GPU → 4; ≤ 9 → 3; ≤ 14 → 2; ≤ 24 → 1; else 0. Little memory or
 * few cores cap the level (`docs/06` §6.1: estimate = f(cores, RAM, GPU class, benchmark)).
 */
export type Hardware = { cores: number; ramMb: number; discreteGpu: boolean };

export function score(hw: Hardware, medianMs: number | null): PerfLevel {
  let level: PerfLevel;
  if (medianMs === null) {
    // No WebGL: the hardware alone, never above «Среднее».
    level = hw.cores >= 8 && hw.ramMb >= 8192 ? 2 : 1;
  } else if (medianMs <= 6 && hw.discreteGpu) level = 4;
  else if (medianMs <= 9) level = 3;
  else if (medianMs <= 14) level = 2;
  else if (medianMs <= 24) level = 1;
  else level = 0;
  if (hw.ramMb < 4096 || hw.cores <= 2) level = Math.min(level, 1) as PerfLevel;
  else if (hw.ramMb < 8192 || hw.cores <= 4) level = Math.min(level, 2) as PerfLevel;
  return level;
}

const VERTEX = `attribute vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }`;
/** Layered sine waves with soft glow — the shape of the «Волны» visualisation's load. */
const FRAGMENT = `precision mediump float;
uniform vec2 r; uniform float t;
void main() {
  vec2 uv = gl_FragCoord.xy / r;
  vec3 c = vec3(0.0);
  for (int i = 0; i < 24; i++) {
    float fi = float(i);
    float y = 0.5 + 0.25 * sin(uv.x * (3.0 + fi * 0.37) + t * (0.6 + fi * 0.05) + fi);
    float d = abs(uv.y - y);
    c += vec3(0.4 + 0.02 * fi, 0.3, 0.6 - 0.01 * fi) * (0.004 / (d + 0.004));
  }
  gl_FragColor = vec4(c / 24.0, 1.0);
}`;

/** Median frame time (ms) of the shader at 1920×1080 over `ms`; null without WebGL. */
export function benchmark(ms = 1500): Promise<number | null> {
  const canvas = document.createElement("canvas");
  canvas.width = 1920;
  canvas.height = 1080;
  const gl = canvas.getContext("webgl", { antialias: false, powerPreference: "high-performance" });
  if (!gl) return Promise.resolve(null);
  const shader = (type: number, src: string) => {
    const s = gl.createShader(type);
    if (!s) return null;
    gl.shaderSource(s, src);
    gl.compileShader(s);
    return gl.getShaderParameter(s, gl.COMPILE_STATUS) ? s : null;
  };
  const vs = shader(gl.VERTEX_SHADER, VERTEX);
  const fs = shader(gl.FRAGMENT_SHADER, FRAGMENT);
  const prog = gl.createProgram();
  if (!vs || !fs || !prog) return Promise.resolve(null);
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return Promise.resolve(null);
  // biome-ignore lint/correctness/useHookAtTopLevel: WebGL call, not a React hook
  gl.useProgram(prog);
  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, "p");
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  gl.uniform2f(gl.getUniformLocation(prog, "r"), 1920, 1080);
  gl.viewport(0, 0, 1920, 1080);
  const tLoc = gl.getUniformLocation(prog, "t");

  return new Promise((resolve) => {
    const frames: number[] = [];
    const start = performance.now();
    const step = () => {
      const t0 = performance.now();
      gl.uniform1f(tLoc, (t0 - start) / 1000);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      // Wait for the GPU: the frame time is what the card needs, not what the CPU queued.
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array(4));
      frames.push(performance.now() - t0);
      if (performance.now() - start < ms) requestAnimationFrame(step);
      else {
        gl.getExtension("WEBGL_lose_context")?.loseContext();
        // The first frames include shader warm-up.
        const s = frames.slice(Math.min(3, frames.length - 1)).sort((a, b) => a - b);
        resolve(s[Math.floor(s.length / 2)] ?? null);
      }
    };
    requestAnimationFrame(step);
  });
}
