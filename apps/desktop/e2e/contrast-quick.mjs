// Contrast of the text over the live visualiser (T7.7, WCAG 1.4.3): what axe cannot measure
// («incomplete» color-contrast — the background is a canvas) is measured from the pixels. For
// each of those text elements, in both themes and in each visualisation mode, five screenshots
// a second apart: the text's own colour against the background pixels in its box (the pixels
// least like the text — the glyphs themselves are left out), the worst decile, the worst frame.
// AA: 4.5:1, large text (≥ 24 px, or ≥ 18.66 px bold) 3:1. Isolated instance, music very quiet.
// SMOKE_PORT=9375 node apps/desktop/e2e/contrast-quick.mjs   (shots: target/contrast/)

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const axePw = createRequire(require.resolve("@axe-core/playwright"));
const axeSource = readFileSync(axePw.resolve("axe-core/axe.min.js"), "utf8");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-contrast-"));
const outDir = join(root, "target", "contrast");
mkdirSync(outDir, { recursive: true });
const PORT = Number(process.env.SMOKE_PORT ?? 9375);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};

const dir = join(dataDir, "in", "Artist", "Album");
mkdirSync(dir, { recursive: true });
for (const n of [1, 2, 3])
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    `sine=frequency=${110 * n}:duration=90,volume=-30dB`,
    "-c:a",
    "libmp3lame",
    "-b:a",
    "64k",
    "-metadata",
    `title=Песня номер ${n}`,
    "-metadata",
    "artist=Исполнитель",
    "-metadata",
    "album=Альбом",
    "-metadata",
    `track=${n}`,
    join(dir, `s${n}.mp3`),
  ]);

const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
  },
  stdio: "ignore",
});
let browser = null;
for (let i = 0; i < 80 && !browser; i++) {
  try {
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
  } catch {
    await sleep(250);
  }
}
let page = null;
for (let i = 0; i < 80 && !page; i++) {
  page =
    browser
      .contexts()
      .flatMap((c) => c.pages())
      .find((p) => !p.url().includes("widget")) ?? null;
  if (!page) await sleep(250);
}
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);

/** Text over the canvas: axe's «incomplete» color-contrast nodes (selectors). */
const overCanvas = async () => {
  await page.evaluate(axeSource);
  return page.evaluate(async () => {
    const r = await window.axe.run(document, { runOnly: ["color-contrast"] });
    return r.incomplete.flatMap((v) => v.nodes.map((n) => n.target.join(" ")));
  });
};

const worst = new Map(); // "theme/mode · selector" → { ratio, need, text }
// One screenshot, measured in the page itself (no PNG decoder here): the shot is decoded with
// createImageBitmap, each element's box read back from a canvas.
const measureShot = (b64, sels) =>
  page.evaluate(
    async ([data, list]) => {
      const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
      const bmp = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      const cv = new OffscreenCanvas(bmp.width, bmp.height);
      const g = cv.getContext("2d");
      g.drawImage(bmp, 0, 0);
      const scale = bmp.width / window.innerWidth;
      const lum = ([r, gr, b]) => {
        const f = (c) => {
          const s = c / 255;
          return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
        };
        return 0.2126 * f(r) + 0.7152 * f(gr) + 0.0722 * f(b);
      };
      const ratio = (a, b) => {
        const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
        return (x + 0.05) / (y + 0.05);
      };
      const out = [];
      for (const sel of list) {
        const e = document.querySelector(sel);
        if (!e) continue;
        const r = e.getBoundingClientRect();
        const cs = getComputedStyle(e);
        let op = 1;
        for (let n = e; n; n = n.parentElement) op *= Number(getComputedStyle(n).opacity);
        if (r.width < 2 || r.height < 2 || op < 0.5) continue;
        const color = cs.color
          .match(/[\d.]+/g)
          .map(Number)
          .slice(0, 3);
        const size = parseFloat(cs.fontSize);
        const large = size >= 24 || (Number(cs.fontWeight) >= 700 && size >= 18.66);
        const x0 = Math.max(0, Math.floor(r.x * scale));
        const y0 = Math.max(0, Math.floor(r.y * scale));
        const w = Math.min(bmp.width - x0, Math.ceil(r.width * scale));
        const h = Math.min(bmp.height - y0, Math.ceil(r.height * scale));
        if (w < 2 || h < 2) continue;
        const d = g.getImageData(x0, y0, w, h).data;
        const px = [];
        for (let i = 0; i < d.length; i += 8) px.push([d[i], d[i + 1], d[i + 2]]);
        const tl = lum(color);
        px.sort((a, b) => Math.abs(lum(b) - tl) - Math.abs(lum(a) - tl));
        const bg = px.slice(0, Math.floor(px.length / 2));
        const rs = bg.map((p) => ratio(color, p)).sort((a, b) => a - b);
        out.push({
          sel,
          ratio: rs[Math.floor(rs.length * 0.1)],
          need: large ? 3 : 4.5,
          text: e.textContent.trim().slice(0, 40),
        });
      }
      return out;
    },
    [b64, sels],
  );
const measure = async (label, sels) => {
  for (let shot = 0; shot < 5; shot++) {
    const b64 = (await page.screenshot()).toString("base64");
    for (const m of await measureShot(b64, sels)) {
      const key = `${label} · ${m.sel}`;
      const prev = worst.get(key);
      if (!prev || m.ratio < prev.ratio) worst.set(key, m);
    }
    await sleep(1000);
  }
};

try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("import_paths", { paths: [join(dataDir, "in", "Artist")] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  await invoke("playback_set_volume", { volume: 0.05 });
  await invoke("wave_start", { profile: null, query: null });
  for (const theme of ["dark", "light"]) {
    await invoke("settings_patch", { patch: { appearance: { theme } } });
    for (const mode of ["waves", "milkdrop", "titles"]) {
      await invoke("settings_patch", { patch: { viz: { mode } } });
      await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
      await sleep(mode === "milkdrop" ? 6000 : 3000);
      const sels = await overCanvas();
      await measure(`${theme}/${mode}`, sels);
      await page.screenshot({ path: join(outDir, `${theme}-${mode}.png`) });
    }
  }
  await invoke("playback_toggle", {});
  for (const [key, w] of [...worst].sort((a, b) => a[1].ratio - b[1].ratio)) {
    check(`${key} «${w.text}»`, w.ratio >= w.need, `${w.ratio.toFixed(2)}:1 (нужно ${w.need}:1)`);
  }
  if (worst.size === 0) check("text over the canvas found", false, "axe listed nothing");
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await browser?.close().catch(() => {});
await sleep(1000);
app.kill();
console.log(failed ? "contrast-quick: FAILED" : "contrast-quick: ok");
process.exit(failed ? 1 : 0);
