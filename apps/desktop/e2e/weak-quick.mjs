// A weak laptop (T7.1, AT-10) in the real app, isolated: no GPU (`--disable-gpu`, WebGL falls back
// to software) and the renderer's CPU slowed 6× through CDP. First proves the slowdown is real
// (a busy loop before and after), then: «Определить автоматически» picks a low level; on
// «Супермощное» over MilkDrop the frames drop and auto-degrade lowers the level, which survives a
// reload until «Вернуть»; screen switches still answer within 400 ms.
// Two cases: WEAK=soft — software WebGL (SwiftShader), slow; WEAK=nogl — no WebGL at all, the
// visualiser must fall back to «Волны» without errors. CDP slows only the renderer's main thread —
// not the Rust core, the audio or the GPU process.
// WEAK=soft SMOKE_PORT=9371 node apps/desktop/e2e/weak-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-weakq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9371);
const RATE = 6;
const WEAK = process.env.WEAK ?? "soft";
const GPU_ARGS = WEAK === "soft" ? "--disable-gpu --enable-unsafe-swiftshader" : "--disable-gpu";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const dir = join(dataDir, "in", "Artist", "Album");
mkdirSync(dir, { recursive: true });
for (const n of [1, 2])
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    `sine=frequency=${300 + n * 40}:duration=120`,
    "-af",
    "volume=-40dB",
    "-c:a",
    "libmp3lame",
    "-b:a",
    "32k",
    "-metadata",
    `title=Song ${n}`,
    "-metadata",
    "artist=Artist",
    "-metadata",
    "album=Sky High",
    "-metadata",
    `track=${n}`,
    join(dir, `s${n}.mp3`),
  ]);
const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT} ${GPU_ARGS}`,
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
const perf = async () => (await invoke("settings_get", {})).performance;
// Iterations of a tight loop in 300 ms of the page's own clock.
const spin = () =>
  page.evaluate(() => {
    let n = 0;
    const t = performance.now();
    while (performance.now() - t < 300) n++;
    return n;
  });
const errors = [];
page.on("console", (m) => {
  if (m.type() === "error") errors.push(m.text());
});
page.on("pageerror", (e) => errors.push(String(e)));
// The same rule as `shared/theme/autodetect.ts` `score()` (FR-PERF-003).
const score = (hw, ms) => {
  let l;
  if (ms === null) l = hw.cores >= 8 && hw.ramMb >= 8192 ? 2 : 1;
  else if (ms <= 6 && hw.discreteGpu) l = 4;
  else if (ms <= 9) l = 3;
  else if (ms <= 14) l = 2;
  else if (ms <= 24) l = 1;
  else l = 0;
  if (hw.ramMb < 4096 || hw.cores <= 2) l = Math.min(l, 1);
  else if (hw.ramMb < 8192 || hw.cores <= 4) l = Math.min(l, 2);
  return l;
};
const cdp = await page.context().newCDPSession(page);
const throttle = (rate) => cdp.send("Emulation.setCPUThrottlingRate", { rate });
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  const gl = await page.evaluate(() => {
    const c = document.createElement("canvas").getContext("webgl");
    const ext = c?.getExtension("WEBGL_debug_renderer_info");
    return c ? String(ext ? c.getParameter(ext.UNMASKED_RENDERER_WEBGL) : "webgl") : "no webgl";
  });
  console.log(`      WebGL without GPU: ${gl}`);

  // —— the slowdown is real ——
  const fast = await spin();
  await throttle(RATE);
  const slow = await spin();
  check(
    `CDP slows the renderer (${RATE}×)`,
    fast / slow >= RATE / 2,
    `${fast} → ${slow} iterations per 300 ms (×${(fast / slow).toFixed(1)})`,
  );

  // —— «Определить автоматически» ——
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  await page.getByRole("button", { name: "Определить автоматически" }).click();
  await page.getByRole("status").filter({ hasText: "Определено" }).waitFor({ timeout: 20000 });
  const detected = (await perf()).level;
  const bench = await page.evaluate(() => window.__sirtoneBench ?? null);
  const want = bench ? score(bench.hw, bench.median) : -1;
  check(
    "detect follows the rule for what it measured (no GPU → never above «Среднее»)",
    detected === want && detected <= 2,
    `level ${detected}, benchmark median ${bench?.median ?? "—"} ms, ${bench?.hw.cores} cores, ${bench?.hw.ramMb} MB`,
  );

  // —— auto-degrade on a weak machine set too high ——
  await invoke("import_paths", { paths: [join(dataDir, "in", "Artist")] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  await invoke("settings_patch", {
    patch: {
      performance: { level: 4, overrides: {}, degraded_from: null, auto_degrade: true },
      viz: { mode: "milkdrop" },
    },
  });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("playback_set_volume", { volume: 0.05 }).catch(() => {});
  await invoke("wave_start", { profile: null, query: null });
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  if (WEAK === "soft")
    await page.locator('canvas[data-testid="milkdrop"]').waitFor({ timeout: 20000 });
  else await sleep(4000);
  const again = await spin();
  check(
    "the slowdown survives page reloads",
    fast / again >= RATE / 2,
    `${again} iterations per 300 ms (×${(fast / again).toFixed(1)})`,
  );
  const p95 = () => page.evaluate(() => window.__sirtoneFrames?.p95() ?? 0);
  await sleep(1500);
  console.log(`      p95 frame on «Супермощное» + MilkDrop: ${(await p95()).toFixed(1)} ms`);
  const t0 = Date.now();
  let level = 4;
  while (Date.now() - t0 < 30000 && level === 4) {
    await sleep(500);
    level = (await perf()).level;
  }
  const p1 = await perf();
  const lowered = p1.level < 4 && p1.degraded_from === 4;
  const how = `level 4 → ${p1.level} in ${Date.now() - t0} ms, degraded_from ${p1.degraded_from}`;
  // Without WebGL there is no heavy drawing: the frames may well stay smooth.
  if (WEAK === "soft") check("slow frames lower the level", lowered, how);
  else console.log(`      without WebGL: ${how}`);
  // Let it settle where it is (one step per ≥ 10 s while still slow).
  await sleep(25000);
  const settled = await perf();
  console.log(`      after 25 s more: level ${settled.level}, p95 ${(await p95()).toFixed(1)} ms`);
  await page.reload();
  await page.waitForSelector("footer");
  await sleep(1000);
  const after = await perf();
  if (lowered)
    check(
      "the lowered level sticks after a restart of the page",
      after.level <= settled.level && after.degraded_from === 4,
      JSON.stringify(after),
    );
  const playing = (await invoke("playback_snapshot", {})).state;
  check("the music plays on", playing === "playing", playing);

  // —— answers still within 400 ms (the screen switch, measured in the page) ——
  const took = [];
  for (const [button, heading] of [
    ["Настройки", "Производительность"],
    ["Моя волна", null],
    ["Настройки", "Производительность"],
    ["Моя волна", null],
    ["Настройки", "Производительность"],
  ]) {
    const t = Date.now();
    await page.getByRole("button", { name: button, exact: true }).first().click();
    if (heading) await page.getByRole("heading", { name: heading }).waitFor();
    else await page.getByTestId("wave-chips").waitFor();
    took.push(Date.now() - t);
  }
  took.sort((a, b) => a - b);
  check(
    `screen switches at ${RATE}× slower CPU: worst ≤ 400 ms`,
    (took.at(-1) ?? 0) <= 400,
    `${took.join(", ")} ms (with the CDP round trip)`,
  );
  check("no errors in the page", errors.length === 0, errors.slice(0, 3).join(" | "));
  await throttle(1);
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await sleep(1000);
app.kill();
console.log(failed ? "weak-quick: FAILED" : "weak-quick: ok");
process.exit(failed ? 1 : 0);
