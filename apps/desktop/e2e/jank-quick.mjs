// The wave screen's frame pacing while music plays (owner feedback 2026-10-20: «дым дёргается
// каждую секунду»): every rAF gap and long task over 15 s, and React commits per second.
// SMOKE_PORT=9357 node apps/desktop/e2e/jank-quick.mjs

import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-jankq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9357);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
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
await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
await page.reload();
await page.waitForSelector("footer");
await invoke("import_paths", { paths: [join(root, "fixtures", "generated", "import500", "Air")] });
await page.getByRole("button", { name: "Продолжить" }).click();
await page.getByRole("status").filter({ hasText: "Музыка добавлена" }).waitFor({ timeout: 60000 });
await invoke("settings_patch", {
  patch: { appearance: { seasonal_theme: process.env.SEASONAL === "1" } },
});
await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
await invoke("wave_start", {});
await invoke("playback_set_repeat", { mode: "all" });
await sleep(3000);
const r = await page.evaluate(
  () =>
    new Promise((resolve) => {
      const gaps = [];
      const longs = [];
      new PerformanceObserver((l) => {
        for (const e of l.getEntries())
          longs.push([Math.round(e.startTime), Math.round(e.duration)]);
      }).observe({ type: "longtask" });
      let last = 0;
      const t0 = performance.now();
      const tick = (now) => {
        if (last && now - last > 25) gaps.push([Math.round(now - t0), Math.round(now - last)]);
        last = now;
        if (now - t0 < 15000) requestAnimationFrame(tick);
        else resolve({ gaps, longs: longs.map(([s, d]) => [Math.round(s - t0), d]) });
      };
      requestAnimationFrame(tick);
    }),
);
console.log(`gaps > 25 ms in 15 s: ${r.gaps.length} ${JSON.stringify(r.gaps.slice(0, 30))}`);
console.log(`long tasks: ${r.longs.length} ${JSON.stringify(r.longs.slice(0, 30))}`);
app.kill();
process.exit(0);
