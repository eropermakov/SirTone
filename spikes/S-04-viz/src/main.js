// S-04 page: receives 60 Hz frames over a Tauri Channel and runs the render phases.
import { Channel, invoke } from "@tauri-apps/api/core";
import butterchurnModule from "butterchurn";
import presetsModule from "butterchurn-presets";

// UMD bundles: the class may sit under `.default`.
const butterchurn = butterchurnModule.default ?? butterchurnModule;
const butterchurnPresets = presetsModule.default ?? presetsModule;

const log = document.getElementById("log");
const say = (s) => {
  log.textContent += `${s}\n`;
  console.log(s);
};
const results = { phases: [] };
window.__S04 = results;

// ---- channel stats ---------------------------------------------------------------------------
let latest = null;
let arrivals = [];
let lat = [];
let seqs = [];
const ch = new Channel();
ch.onmessage = (buf) => {
  const now = performance.now();
  const v = new DataView(buf);
  arrivals.push(now);
  seqs.push(v.getUint32(24, true));
  lat.push(now * 1000 - v.getUint32(28, true)); // clock offset unknown; the spread matters
  latest = buf;
};
const resetStats = () => {
  arrivals = [];
  lat = [];
  seqs = [];
};
const pct = (a, p) => {
  const s = [...a].sort((x, y) => x - y);
  return s[Math.min(s.length - 1, Math.floor(s.length * p))];
};
function channelStats() {
  const d = arrivals.slice(1).map((t, i) => t - arrivals[i]);
  const jitter = d.map((x) => Math.abs(x - 1000 / 60));
  let lost = 0;
  for (let i = 1; i < seqs.length; i++) lost += Math.max(0, seqs[i] - seqs[i - 1] - 1);
  const base = Math.min(...lat);
  const rel = lat.map((x) => (x - base) / 1000);
  return {
    frames: seqs.length,
    lost,
    jitterP50: +pct(jitter, 0.5).toFixed(2),
    jitterP99: +pct(jitter, 0.99).toFixed(2),
    jitterMax: +Math.max(...jitter).toFixed(2),
    relLatencyP50: +pct(rel, 0.5).toFixed(2),
    relLatencyP99: +pct(rel, 0.99).toFixed(2),
  };
}

// ---- render loop helpers ---------------------------------------------------------------------
let canvas = document.getElementById("c");
function runFrames(seconds, draw) {
  return new Promise((done) => {
    const dts = [];
    let last = performance.now();
    const t0 = last;
    const tick = (now) => {
      dts.push(now - last);
      last = now;
      draw(now);
      if (now - t0 < seconds * 1000) requestAnimationFrame(tick);
      else done(dts.slice(10));
    };
    requestAnimationFrame(tick);
  });
}
function frameStats(dts) {
  const avg = dts.reduce((s, x) => s + x, 0) / dts.length;
  return {
    fps: +(1000 / avg).toFixed(1),
    p95ms: +pct(dts, 0.95).toFixed(1),
    over25: dts.filter((x) => x > 25).length,
    renderFrames: dts.length,
  };
}
const levels = () =>
  latest && {
    timeByteArray: new Uint8Array(latest, 32, 1024),
    timeByteArrayL: new Uint8Array(latest, 32 + 1024, 1024),
    timeByteArrayR: new Uint8Array(latest, 32 + 2048, 1024),
  };

// ---- fBm "Waves" stand-in: 2-pass domain warp, 4 colours (docs/06 §7.2) -----------------------
function wavesRenderer(gl) {
  const vs = "#version 300 es\nin vec2 p; void main(){ gl_Position = vec4(p,0.,1.); }";
  const fs = `#version 300 es
precision highp float; uniform vec2 r; uniform float t; uniform vec3 bands; out vec4 o;
float h(vec2 p){ return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453); }
float n(vec2 p){ vec2 i=floor(p),f=fract(p); f=f*f*(3.-2.*f);
  return mix(mix(h(i),h(i+vec2(1,0)),f.x),mix(h(i+vec2(0,1)),h(i+vec2(1,1)),f.x),f.y); }
float fbm(vec2 p){ float v=0.,a=.5; for(int i=0;i<6;i++){ v+=a*n(p); p*=2.02; a*=.5; } return v; }
void main(){ vec2 uv=gl_FragCoord.xy/r.y; float s=.08*t;
  vec2 q=vec2(fbm(uv+s),fbm(uv+vec2(5.2,1.3)-s));
  vec2 w=vec2(fbm(uv+4.*q+vec2(1.7,9.2)+.15*s),fbm(uv+4.*q+vec2(8.3,2.8)-.12*s));
  float f=fbm(uv+(3.+2.*bands.x)*w);
  vec3 c=mix(vec3(.10,.06,.25),vec3(.55,.10,.35),clamp(f*f*2.,0.,1.));
  c=mix(c,vec3(.95,.55,.25),clamp(length(q)*.6,0.,1.));
  c=mix(c,vec3(.2,.7,.75),clamp(w.x*w.x,0.,1.)*bands.z);
  c*=1.-.35*length(gl_FragCoord.xy/r-.5); o=vec4(c,1.); }`;
  const sh = (type, src) => {
    const s = gl.createShader(type);
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    return s;
  };
  const pr = gl.createProgram();
  gl.attachShader(pr, sh(gl.VERTEX_SHADER, vs));
  gl.attachShader(pr, sh(gl.FRAGMENT_SHADER, fs));
  gl.linkProgram(pr);
  gl.useProgram(pr);
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(pr, "p");
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  const ur = gl.getUniformLocation(pr, "r");
  const ut = gl.getUniformLocation(pr, "t");
  const ub = gl.getUniformLocation(pr, "bands");
  return (now) => {
    let bass = 0.5;
    let treb = 0.5;
    if (latest) {
      const v = new DataView(latest);
      bass = v.getFloat32(18, true);
      treb = v.getFloat32(26, true);
    }
    gl.viewport(0, 0, 1920, 1080);
    gl.uniform2f(ur, 1920, 1080);
    gl.uniform1f(ut, now / 1000);
    gl.uniform3f(ub, bass, 0.5, treb);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  };
}

async function main() {
  await invoke("start_stream", { channel: ch });
  say("phase A: channel only, 10 s");
  await new Promise((r) => setTimeout(r, 1000));
  resetStats();
  const a = await runFrames(10, () => {});
  results.phases.push({ name: "channel only", ...channelStats(), ...frameStats(a) });
  say(JSON.stringify(results.phases.at(-1)));

  const presets = butterchurnPresets.getPresets();
  const names = Object.keys(presets);
  results.presetCount = names.length;
  const pick = [
    "Flexi, martin + geiss - dedicated to the sherwin maxawow",
    "Geiss - Cosmic Dust 2",
    "Martin - liquid arrows",
  ].filter((n) => presets[n]);
  for (let k = 0; pick.length < 3; k++) pick.push(names[(k * 97) % names.length]);
  results.presets = pick;
  for (const [label, mw, mh] of [
    ["MilkDrop medium (mesh 32x24)", 32, 24],
    ["MilkDrop high (mesh 48x36)", 48, 36],
  ]) {
    const viz = butterchurn.createVisualizer(null, canvas, {
      width: 1920,
      height: 1080,
      meshWidth: mw,
      meshHeight: mh,
      pixelRatio: 1,
      textureRatio: 1,
    });
    say(`phase: ${label}`);
    viz.loadPreset(presets[pick[0]], 0);
    resetStats();
    let i = 0;
    const swap = setInterval(() => {
      i = (i + 1) % pick.length;
      viz.loadPreset(presets[pick[i]], 2.0);
    }, 5000);
    const d = await runFrames(15, () => viz.render({ audioLevels: levels() }));
    clearInterval(swap);
    results.phases.push({ name: label, ...channelStats(), ...frameStats(d) });
    say(JSON.stringify(results.phases.at(-1)));
  }
  const fresh = document.createElement("canvas");
  fresh.width = 1920;
  fresh.height = 1080;
  canvas.replaceWith(fresh);
  canvas = fresh;
  const draw = wavesRenderer(fresh.getContext("webgl2"));
  say("phase: waves fBm 1080p, 10 s");
  resetStats();
  const w = await runFrames(10, draw);
  results.phases.push({ name: "Waves (fBm stand-in) 1080p", ...channelStats(), ...frameStats(w) });
  say(JSON.stringify(results.phases.at(-1)));
  await invoke("stop_stream");
  results.done = true;
  say("done");
}
main().catch((e) => {
  results.error = String(e?.stack ?? e);
  say(`ERROR ${results.error}`);
});
