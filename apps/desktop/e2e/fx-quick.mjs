// Live effects (T4.19 spike S-19 measurements + T4.20 smoke), an isolated app with music:
// frame p95 of the wave screen with Волны / MilkDrop / Видео, without effects and with 300 /
// 1500 / 5000 particles; the effects' own GPU time (timer query if present, else gl.finish);
// the number of GL contexts; the settings path (manual pick → the type on screen); the layer
// is gone outside the wave screen.
// SMOKE_PORT=9354 node apps/desktop/e2e/fx-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-fxq-"));
const libDir = join(dataDir, "Music", "SirTone");
const PORT = Number(process.env.SMOKE_PORT ?? 9354);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
    SIRTONE_SMOKE_AC: "1",
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
  },
  stdio: "ignore",
});
let page;
for (let i = 0; i < 80 && !page; i++) {
  try {
    const b = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
    page = b.contexts()[0]?.pages()[0];
  } catch {
    await sleep(250);
  }
}
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);

/** rAF frame times for `ms`: p95 and mean. */
const frames = (ms = 5000) =>
  page.evaluate(
    (dur) =>
      new Promise((resolve) => {
        const d = [];
        let last = 0;
        const t0 = performance.now();
        const tick = (now) => {
          if (last) d.push(now - last);
          last = now;
          if (now - t0 < dur) requestAnimationFrame(tick);
          else {
            d.sort((a, b) => a - b);
            const p95 = d[Math.floor(d.length * 0.95)] ?? 0;
            const mean = d.reduce((a, b) => a + b, 0) / (d.length || 1);
            resolve({ n: d.length, p95: +p95.toFixed(1), mean: +mean.toFixed(1) });
          }
        };
        requestAnimationFrame(tick);
      }),
    ms,
  );
const force = (plan) => page.evaluate((p) => window.__sirtoneFx?.force(p), plan);
const snow = (count, planes = 1) => ({
  type: "snow",
  count,
  fps: 60,
  planes,
  still: false,
  flash: false,
});

try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("import_paths", {
    paths: [join(root, "fixtures", "generated", "import500", "Air")],
  });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
  await invoke("wave_start", {});
  await page.locator('canvas[data-testid="fx"]').waitFor({ timeout: 10000 });
  await sleep(1500);

  const env = await page.evaluate(() => {
    const c = document.createElement("canvas");
    const gl = c.getContext("webgl2");
    const dbg = gl?.getExtension("WEBGL_debug_renderer_info");
    const r = {
      renderer: dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : "n/a",
      timerQuery: !!gl?.getExtension("EXT_disjoint_timer_query_webgl2"),
      dpr: window.devicePixelRatio,
    };
    gl?.getExtension("WEBGL_lose_context")?.loseContext();
    return r;
  });
  const fxCanvas = await page.evaluate(() => {
    const c = document.querySelector('canvas[data-testid="fx"]');
    return c ? `${c.width}×${c.height}` : "none";
  });
  console.log(`ENV  ${JSON.stringify({ ...env, fxCanvas })}`);
  const season = await page.evaluate(() => window.__sirtoneFx?.stats.type);
  check("«По реальной погоде» without weather shows the season's effect (AT-16)", !!season, season);

  const rows = [];
  const measure = async (mode, label, plan) => {
    await force(plan);
    await sleep(1500);
    const f = await frames(5000);
    const gpu = plan ? await page.evaluate(() => window.__sirtoneFx?.probe(60)) : null;
    const contexts = await page.evaluate(
      () =>
        document.querySelectorAll(
          'canvas[data-testid="fx"], canvas[data-testid="waves"], canvas[data-testid="milkdrop"], canvas[data-testid="titles"]',
        ).length,
    );
    rows.push({
      mode,
      label,
      ...f,
      gpuMs: gpu === null ? "—" : +gpu.ms.toFixed(3),
      method: gpu?.method ?? "—",
      canvases: contexts,
    });
  };
  for (const mode of ["waves", "milkdrop", "video", "titles"]) {
    if (mode === "video") {
      const bgDir = join(libDir, ".sirtone", "backgrounds");
      mkdirSync(bgDir, { recursive: true });
      const clipId = "00000000000000aa.webm";
      spawnSync("ffmpeg", [
        "-y",
        "-v",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=1280x720:rate=30",
        "-t",
        "4",
        "-c:v",
        "libvpx-vp9",
        "-b:v",
        "1M",
        join(bgDir, clipId),
      ]);
      writeFileSync(
        join(bgDir, "index.json"),
        JSON.stringify([{ id: clipId, mood: "any", source: "q:1", bytes: 1 }]),
      );
      await page.evaluate(() => window.dispatchEvent(new Event("sirtone-videos-refresh")));
    }
    await invoke("settings_patch", { patch: { viz: { mode } } });
    await sleep(mode === "milkdrop" ? 5000 : 2500);
    await measure(mode, "no fx", null);
    if (mode === "titles") continue; // the scene is its own: no live effects over it (D-122)
    await measure(mode, "300", snow(300));
    await measure(mode, "1500", snow(1500));
    await measure(mode, "5000 ×3", snow(5000, 3));
    if (mode === "waves") {
      const as = (type, count, planes = 1) => ({ ...snow(count, planes), type });
      await measure(mode, "leaves 5000 ×3", as("leaves", 5000, 3));
      await measure(mode, "rain 1500", as("rain", 1500));
      await measure(mode, "fog ¼", as("fog", 0));
    }
  }
  await force(undefined);
  console.table(rows);
  const worst = Math.max(...rows.filter((r) => r.label !== "5000 ×3").map((r) => r.p95));
  check("frame p95 ≤ 20 ms with effects at levels ≤ 3 (P-18)", worst <= 20, `worst ${worst} ms`);

  // Optional screenshots for a look (FX_SHOTS=<dir>).
  if (process.env.FX_SHOTS) {
    await invoke("settings_patch", { patch: { viz: { mode: "waves" } } });
    for (const type of ["leaves", "snow", "petals", "fireflies", "rain", "fog"]) {
      await force({ ...snow(1500), type, count: type === "fog" ? 0 : 1500 });
      await sleep(2500);
      await page.screenshot({ path: join(process.env.FX_SHOTS, `fx-${type}.png`) });
    }
    await force(undefined);
  }

  // The settings path: a manual pick reaches the screen; fog draws at ¼ resolution.
  await invoke("settings_patch", {
    patch: { viz: { mode: "waves" }, fx: { mode: "manual", manual: "fog" } },
  });
  await sleep(3500); // the type change cross-fades over transition_ms
  const fog = await page.evaluate(() => {
    const c = document.querySelector('canvas[data-testid="fx"]');
    return { type: window.__sirtoneFx?.stats.type, w: c?.width, cw: c?.clientWidth };
  });
  check(
    "«Выбрать вручную» → fog, drawn at ¼ resolution",
    fog.type === "fog" && fog.w < fog.cw,
    JSON.stringify(fog),
  );
  await invoke("settings_patch", { patch: { fx: { mode: "off" } } });
  await sleep(800);
  check("«Выкл» — no layer", (await page.locator('canvas[data-testid="fx"]').count()) === 0);
  await invoke("settings_patch", { patch: { fx: { mode: "season" } } });
  await sleep(800);
  await page.getByRole("navigation").getByRole("button", { name: "Настройки" }).click();
  await sleep(800);
  check(
    "no effects outside the wave screen",
    (await page.locator('canvas[data-testid="fx"]').count()) === 0,
  );
} catch (e) {
  check("run", false, String(e).slice(0, 400));
}
app.kill();
process.exit(failed ? 1 : 0);
