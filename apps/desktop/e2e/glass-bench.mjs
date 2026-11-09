// S-21 (T9.0, D-156): cost of CSS `backdrop-filter: blur(26px)` panels over a live WebGL canvas.
// Edge (Chromium) is the engine of WebView2, so the page cost is comparable; window Mica/Acrylic
// (DWM) is NOT measured here. Panels: sidebar 280 wide full height, player 84, header 56.
//   node apps/desktop/e2e/glass-bench.mjs   → JSON lines + table (docs/reports/spike-S-21.md)
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");

const VARIANTS = [
  { name: "D solid (fallback)", blur: 0, alpha: 1 },
  { name: "B blur 26, 97 % matte", blur: 26, alpha: 0.97 },
  { name: "B blur 26, 78 % medium", blur: 26, alpha: 0.78 },
  { name: "B blur 26, 55 % transparent", blur: 26, alpha: 0.55 },
];
const SIZES = [
  [1280, 800],
  [1920, 1080],
];
const SECONDS = Number(process.env.SECONDS ?? 8);

const html = (v) => `<!doctype html><meta charset=utf-8><style>
html,body{margin:0;height:100%;background:#000;overflow:hidden}
canvas{position:fixed;inset:0;width:100%;height:100%}
.p{position:fixed;background:rgba(11,11,11,${v.alpha});${v.blur ? `backdrop-filter:blur(${v.blur}px);-webkit-backdrop-filter:blur(${v.blur}px);` : ""}}
#side{left:0;top:0;bottom:0;width:280px}
#head{left:280px;right:0;top:0;height:56px}
#play{left:280px;right:0;bottom:0;height:84px}
</style><canvas id=c></canvas><div class=p id=side></div><div class=p id=head></div><div class=p id=play></div>
<script>
const c=document.getElementById('c');const gl=c.getContext('webgl2',{antialias:false});
c.width=innerWidth;c.height=innerHeight;gl.viewport(0,0,c.width,c.height);
const vs='#version 300 es\\nin vec2 p;void main(){gl_Position=vec4(p,0,1);}';
const fs='#version 300 es\\nprecision highp float;uniform float t;uniform vec2 r;out vec4 o;void main(){vec2 u=gl_FragCoord.xy/r;float a=0.;for(int i=0;i<24;i++){float f=float(i)+1.;a+=sin(u.x*f*3.+t+f)*cos(u.y*f*2.-t*0.7);}o=vec4(.5+.5*sin(a+t),.5+.5*sin(a*1.3),.5+.5*cos(a+u.x*3.),1);}';
const sh=(t,s)=>{const h=gl.createShader(t);gl.shaderSource(h,s);gl.compileShader(h);return h};
const pg=gl.createProgram();gl.attachShader(pg,sh(gl.VERTEX_SHADER,vs));gl.attachShader(pg,sh(gl.FRAGMENT_SHADER,fs));gl.linkProgram(pg);gl.useProgram(pg);
const b=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,b);gl.bufferData(gl.ARRAY_BUFFER,new Float32Array([-1,-1,3,-1,-1,3]),gl.STATIC_DRAW);
gl.enableVertexAttribArray(0);gl.vertexAttribPointer(0,2,gl.FLOAT,false,0,0);
const tl=gl.getUniformLocation(pg,'t');gl.uniform2f(gl.getUniformLocation(pg,'r'),c.width,c.height);
window.__d=[];let last=0;
function f(ts){gl.uniform1f(tl,ts/1000);gl.drawArrays(gl.TRIANGLES,0,3);if(last)window.__d.push(ts-last);last=ts;requestAnimationFrame(f)}
requestAnimationFrame(f);
</script>`;

const pct = (a, p) => a[Math.min(a.length - 1, Math.floor((a.length * p) / 100))];
const browser = await chromium.launch({ channel: "msedge", headless: false });
const rows = [];
for (const [w, h] of SIZES) {
  for (const v of VARIANTS) {
    const page = await browser.newPage({ viewport: { width: w, height: h } });
    await page.setContent(html(v));
    await page.waitForTimeout(1500);
    await page.evaluate(() => {
      window.__d.length = 0;
    });
    await page.waitForTimeout(SECONDS * 1000);
    const d = await page.evaluate(() => window.__d.slice());
    const s = [...d].sort((a, b) => a - b);
    const mean = d.reduce((a, b) => a + b, 0) / d.length;
    rows.push({
      size: `${w}x${h}`,
      variant: v.name,
      frames: d.length,
      fps: +(1000 / mean).toFixed(1),
      p50: +pct(s, 50).toFixed(1),
      p95: +pct(s, 95).toFixed(1),
      p99: +pct(s, 99).toFixed(1),
      over20: d.filter((x) => x > 20).length,
    });
    await page.close();
  }
}
await browser.close();
console.log("| size | variant | frames | fps | p50 ms | p95 ms | p99 ms | frames > 20 ms |");
console.log("|---|---|---|---|---|---|---|---|");
for (const r of rows)
  console.log(
    `| ${r.size} | ${r.variant} | ${r.frames} | ${r.fps} | ${r.p50} | ${r.p95} | ${r.p99} | ${r.over20} |`,
  );
