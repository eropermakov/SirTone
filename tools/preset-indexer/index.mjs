// MilkDrop preset indexer (T4.3, `06` §7.3, dev tool — not shipped). Renders every preset of
// `butterchurn-presets` headless in Edge with synthetic sound at three intensities and measures
// what the preset looks like: hue, saturation, brightness, motion, reactivity, chaos — and the
// flashes (WCAG 2.3.1 general flash: a ≥ 10 % luminance swing over ≥ 25 % of the picture, there
// and back). Presets that flash, stay black or stand still are left out. Writes:
//   assets/viz/milkdrop/index.json   — descriptors of the kept presets (the vibe picks by them)
//   assets/viz/milkdrop/presets.js   — the kept presets with their equations as real functions
//                                      (the app's CSP forbids `new Function`, D-95)
//   assets/viz/milkdrop/CREDITS.md   — authors (preset names carry them; presets are community
//                                      works shipped in the MIT `butterchurn-presets` package)
//   target/preset-index-all.json     — every preset with its numbers (for review)
//
// node tools/preset-indexer/index.mjs <butterchurn/lib dir> <butterchurn-presets/presets/converted>
//   [--limit N] [--keep 500]

import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const root = join(import.meta.dirname, "..", "..");
const require = createRequire(join(root, "apps", "desktop", "package.json"));
const { chromium } = require("@playwright/test");

const [libDir, presetsDir] = process.argv.slice(2);
if (!libDir || !presetsDir) {
  console.error("usage: index.mjs <butterchurn lib dir> <presets/converted dir> [--limit N]");
  process.exit(2);
}
const arg = (name, d) => {
  const i = process.argv.indexOf(name);
  return i > 0 ? Number(process.argv[i + 1]) : d;
};
const LIMIT = arg("--limit", Infinity);
const KEEP = arg("--keep", 500);

const REUSE = process.argv.includes("--reuse");
const files = readdirSync(presetsDir)
  .filter((f) => f.endsWith(".json"))
  .sort()
  .slice(0, LIMIT);

const started = Date.now();
const all = REUSE
  ? JSON.parse(readFileSync(join(root, "target", "preset-index-all.json"), "utf8"))
  : await measureAll();

async function measureAll() {
  const browser = await chromium.launch({
    channel: "msedge",
    headless: true,
    args: ["--use-angle=d3d11", "--enable-gpu", "--ignore-gpu-blocklist"],
  });
  const page = await browser.newPage();
  await page.setContent("<canvas id=c width=256 height=144></canvas>");
  await page.addScriptTag({ path: join(libDir, "butterchurn.min.js") });

  // The measuring routine, in the page: one preset, three intensities, 150 frames each at 60 fps,
  // sampled every 3rd frame (20 Hz) on a 32×18 grid of blocks.
  await page.evaluate(() => {
    const bc = window.butterchurn.default ?? window.butterchurn;
    const canvas = document.getElementById("c");
    const W = 256;
    const H = 144;
    const viz = bc.createVisualizer(null, canvas, {
      width: W,
      height: H,
      meshWidth: 32,
      meshHeight: 24,
      pixelRatio: 1,
      textureRatio: 1,
    });
    const gl = canvas.getContext("webgl2");
    const px = new Uint8Array(W * H * 4);
    const BX = 32;
    const BY = 18;
    const lin = (v) => {
      const c = v / 255;
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    };
    const LUT = Float32Array.from({ length: 256 }, (_, i) => lin(i));
    // Synthetic music: a kick at 120 bpm, a bass line, a mid tone and noise, scaled by intensity.
    const audio = (t, k) => {
      const a = new Uint8Array(1024);
      const beat = (t * 2) % 1;
      const kick = Math.exp(-beat * 12);
      for (let i = 0; i < 1024; i++) {
        const s = t + i / 48000;
        const v =
          k *
          (0.6 * kick * Math.sin(2 * Math.PI * 55 * s) +
            0.25 * Math.sin(2 * Math.PI * 110 * s) +
            0.12 * Math.sin(2 * Math.PI * 880 * s) +
            0.15 * (Math.random() * 2 - 1));
        a[i] = Math.max(0, Math.min(255, Math.round(128 + 127 * v)));
      }
      return a;
    };
    window.measure = (preset) => {
      viz.loadPreset(preset, 0);
      const out = { runs: [], flashes: 0 };
      let t = 0;
      for (const k of [0.15, 0.5, 1.0]) {
        let prev = null;
        let lastDir = 0;
        let lastAt = -1e9;
        let motion = 0;
        let motionSq = 0;
        let bright = 0;
        let hx = 0;
        let hy = 0;
        let sat = 0;
        let n = 0;
        for (let f = 0; f < 150; f++) {
          const lv = audio(t, k);
          viz.render({
            audioLevels: { timeByteArray: lv, timeByteArrayL: lv, timeByteArrayR: lv },
            elapsedTime: 1 / 60,
          });
          t += 1 / 60;
          if (f % 3 !== 2) continue;
          gl.readPixels(0, 0, W, H, gl.RGBA, gl.UNSIGNED_BYTE, px);
          const blocks = new Float32Array(BX * BY);
          let sumL = 0;
          let s = 0;
          let x = 0;
          let y = 0;
          for (let by = 0; by < BY; by++) {
            for (let bx = 0; bx < BX; bx++) {
              let acc = 0;
              for (let yy = 0; yy < 8; yy++) {
                for (let xx = 0; xx < 8; xx++) {
                  const i = ((by * 8 + yy) * W + bx * 8 + xx) * 4;
                  const r = px[i];
                  const g = px[i + 1];
                  const b = px[i + 2];
                  acc += 0.2126 * LUT[r] + 0.7152 * LUT[g] + 0.0722 * LUT[b];
                  if ((xx & 3) === 0 && (yy & 3) === 0) {
                    const mx = Math.max(r, g, b);
                    const mn = Math.min(r, g, b);
                    if (mx > 20) {
                      const sv = (mx - mn) / mx;
                      let h = 0;
                      if (mx !== mn) {
                        if (mx === r) h = ((g - b) / (mx - mn)) % 6;
                        else if (mx === g) h = (b - r) / (mx - mn) + 2;
                        else h = (r - g) / (mx - mn) + 4;
                      }
                      const ang = (h / 6) * 2 * Math.PI;
                      x += Math.cos(ang) * sv;
                      y += Math.sin(ang) * sv;
                      s += sv;
                    }
                  }
                }
              }
              blocks[by * BX + bx] = acc / 64;
              sumL += acc / 64;
            }
          }
          const meanL = sumL / (BX * BY);
          bright += meanL;
          sat += s / (BX * BY * 4 || 1);
          hx += x;
          hy += y;
          if (prev) {
            let d = 0;
            let up = 0;
            let down = 0;
            for (let i = 0; i < blocks.length; i++) {
              const delta = blocks[i] - prev[i];
              d += Math.abs(delta);
              const darker = Math.min(blocks[i], prev[i]);
              if (Math.abs(delta) >= 0.1 && darker < 0.8) {
                if (delta > 0) up++;
                else down++;
              }
            }
            d /= blocks.length;
            motion += d;
            motionSq += d * d;
            // A transition: ≥ 25 % of the picture swings the same way; a flash — a pair of
            // opposite transitions within one second.
            const area = blocks.length * 0.25;
            const dir = up >= area ? 1 : down >= area ? -1 : 0;
            if (dir !== 0) {
              if (lastDir === -dir && t - lastAt <= 1) out.flashes++;
              lastDir = dir;
              lastAt = t;
            }
          }
          prev = blocks;
          n++;
        }
        const m = motion / Math.max(1, n - 1);
        out.runs.push({
          k,
          bright: bright / n,
          sat: sat / n,
          hue: ((Math.atan2(hy, hx) * 180) / Math.PI + 360) % 360,
          motion: m,
          chaos: Math.sqrt(Math.max(0, motionSq / Math.max(1, n - 1) - m * m)),
        });
      }
      return out;
    };
  });

  const all = [];
  for (const [i, f] of files.entries()) {
    const name = f.replace(/\.json$/, "");
    const preset = JSON.parse(readFileSync(join(presetsDir, f), "utf8"));
    let m = null;
    try {
      m = await page.evaluate((p) => window.measure(p), preset);
    } catch (e) {
      all.push({ name, error: String(e).slice(0, 200) });
      continue;
    }
    const [quiet, mid, loud] = m.runs;
    const r = (x) => Math.round(x * 1000) / 1000;
    all.push({
      name,
      author: name
        .split(" - ")[0]
        .replace(/^[^A-Za-z0-9]+/, "")
        .trim(),
      hue: r(mid.hue),
      sat: r(mid.sat),
      bright: r((quiet.bright + mid.bright + loud.bright) / 3),
      motion: r(mid.motion),
      react: r(loud.motion - quiet.motion),
      chaos: r(mid.chaos),
      flashes: m.flashes,
    });
    if (i % 50 === 0) {
      const s = (Date.now() - started) / 1000;
      console.log(`${i + 1}/${files.length} ${s.toFixed(0)} s`);
    }
  }
  await browser.close();
  return all;
}

// Kept: no flashes, not black, not frozen, renders. The rest are listed with the reason.
const ok = all.filter((p) => !p.error && p.flashes === 0 && p.bright >= 0.02 && p.motion >= 0.0015);
// Spread the kept set over the descriptor space: sort by (hue bucket, motion) and thin evenly.
ok.sort((a, b) => Math.floor(a.hue / 30) - Math.floor(b.hue / 30) || a.motion - b.motion);
const step = Math.max(1, ok.length / KEEP);
const kept = [];
for (let x = 0; x < ok.length && kept.length < KEEP; x += step) kept.push(ok[Math.floor(x)]);
kept.sort((a, b) => a.name.localeCompare(b.name));

mkdirSync(join(root, "target"), { recursive: true });
writeFileSync(join(root, "target", "preset-index-all.json"), JSON.stringify(all, null, 1));
const outDir = join(root, "assets", "viz", "milkdrop");
mkdirSync(outDir, { recursive: true });
writeFileSync(
  join(outDir, "index.json"),
  `${JSON.stringify(
    {
      _doc: "MilkDrop presets kept by tools/preset-indexer (T4.3): descriptors measured headless with synthetic sound. hue°, sat/bright/motion/react/chaos ≈ 0…1 scale of the measurement; all kept presets have 0 flashes (WCAG 2.3.1).",
      source: "butterchurn-presets 2.4.7 (MIT)",
      measured: all.length,
      kept: kept.length,
      presets: kept.map(({ flashes, ...p }) => p),
    },
    null,
    1,
  )}\n`,
);

// The presets as a module with real functions (no `new Function` at run time).
// `init/frame` must always be functions (Butterchurn compiles a preset whose `init_eqs` is not a
// function with `new Function`); empty `pixel/point` stay '' (Butterchurn's «none»).
const fn = (src) => `function (a) { ${src ?? ""} return a; }`;
const opt = (src) => (src ? fn(src) : "''");
const lines = [
  "// Generated by tools/preset-indexer (T4.3) from butterchurn-presets 2.4.7 (MIT; presets are",
  "// community works, see CREDITS.md). Equations are compiled ahead: the app's CSP has no",
  "// 'unsafe-eval' (D-95). Do not edit by hand.",
  "/* eslint-disable */",
  "export default {",
];
for (const p of kept) {
  const src = JSON.parse(readFileSync(join(presetsDir, `${p.name}.json`), "utf8"));
  const shapes = (src.shapes ?? []).map((s) =>
    s.baseVals?.enabled === 0
      ? JSON.stringify(s)
      : `Object.assign(${JSON.stringify(s)}, { init_eqs: ${fn(s.init_eqs_str)}, frame_eqs: ${fn(s.frame_eqs_str)} })`,
  );
  const waves = (src.waves ?? []).map((w) =>
    w.baseVals?.enabled === 0
      ? JSON.stringify(w)
      : `Object.assign(${JSON.stringify(w)}, { init_eqs: ${fn(w.init_eqs_str)}, frame_eqs: ${fn(w.frame_eqs_str)}, point_eqs: ${opt(w.point_eqs_str)} })`,
  );
  const { shapes: _s, waves: _w, ...rest } = src;
  lines.push(
    `${JSON.stringify(p.name)}: Object.assign(${JSON.stringify(rest)}, { init_eqs: ${fn(src.init_eqs_str)}, frame_eqs: ${fn(src.frame_eqs_str)}, pixel_eqs: ${opt(src.pixel_eqs_str)}, shapes: [${shapes.join(", ")}], waves: [${waves.join(", ")}] }),`,
  );
}
lines.push("};", "");
writeFileSync(join(outDir, "presets.js"), lines.join("\n"));

const authors = new Map();
for (const p of kept) authors.set(p.author, (authors.get(p.author) ?? 0) + 1);
writeFileSync(
  join(outDir, "CREDITS.md"),
  [
    "# Пресеты MilkDrop — авторы",
    "",
    `Пресеты — работы сообщества MilkDrop, взяты из пакета \`butterchurn-presets\` 2.4.7 (MIT, Jordan Berg). Отобрано ${kept.length} из ${all.length}: без вспышек (WCAG 2.3.1), не чёрные, с движением (\`tools/preset-indexer\`). Автор — по названию пресета.`,
    "",
    "| Автор | Пресетов |",
    "|---|---|",
    ...[...authors.entries()].sort((a, b) => b[1] - a[1]).map(([a, n]) => `| ${a || "—"} | ${n} |`),
    "",
  ].join("\n"),
);
const dropped = {
  error: all.filter((p) => p.error).length,
  flashes: all.filter((p) => !p.error && p.flashes > 0).length,
  dark: all.filter((p) => !p.error && p.flashes === 0 && p.bright < 0.02).length,
  still: all.filter((p) => !p.error && p.flashes === 0 && p.bright >= 0.02 && p.motion < 0.0015)
    .length,
};
console.log(
  `measured ${all.length}, kept ${kept.length} of ${ok.length} clean; dropped ${JSON.stringify(dropped)}; ${((Date.now() - started) / 1000).toFixed(0)} s`,
);
