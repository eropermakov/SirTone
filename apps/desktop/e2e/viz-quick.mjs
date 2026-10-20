// Quick visualiser smoke (T4.3/T4.4 debugging): an isolated app, one artist folder imported,
// the wave screen in each mode. The full run is real-app.mjs.
// SMOKE_PORT=9348 node apps/desktop/e2e/viz-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-vizq-"));
const libDir = join(dataDir, "Music", "SirTone");
const PORT = Number(process.env.SMOKE_PORT ?? 9348);
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
const log = [];
page.on("console", (m) => log.push(`${m.type()}: ${m.text().slice(0, 300)}`));
page.on("pageerror", (e) => log.push(`pageerror: ${String(e.stack ?? e).slice(0, 1500)}`));
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
await page.reload();
await page.waitForSelector("footer");
await invoke("import_paths", { paths: [join(root, "fixtures", "generated", "import500", "Air")] });
await page.getByRole("button", { name: "Продолжить" }).click();
await page.getByRole("status").filter({ hasText: "Музыка добавлена" }).waitFor({ timeout: 60000 });
await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
await invoke("wave_start", {});
await sleep(1500);
await invoke("settings_patch", { patch: { viz: { mode: "milkdrop" } } });
await page.locator('canvas[data-testid="milkdrop"]').waitFor({ timeout: 10000 });
await sleep(6000);
console.log(
  "milkdrop",
  JSON.stringify(
    await page.evaluate(() => ({
      stats: window.__sirtoneMilkdrop?.stats,
      err: window.__sirtoneMilkdropError,
      power: window.__sirtoneLastPower,
      csp: window.__sirtoneCspErrors,
    })),
  ),
);
const st = () => page.evaluate(() => window.__sirtoneMilkdrop?.stats.preset);
console.log("before", await st());
await page.getByRole("button", { name: "Следующий пресет" }).click();
await sleep(500);
console.log("after next", await st());
await page.getByRole("button", { name: "Больше не показывать этот пресет" }).click();
await sleep(800);
console.log("after no", await st());
const { readFileSync, existsSync } = await import("node:fs");
const rf = join(dataDir, "viz-presets.json");
console.log("rated", existsSync(rf) ? readFileSync(rf, "utf8") : "none");
console.log("canvases", await page.locator('canvas[data-testid="milkdrop"]').count());
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
  "testsrc2=size=320x180:rate=30",
  "-t",
  "3",
  "-c:v",
  "libvpx-vp9",
  "-b:v",
  "300k",
  join(bgDir, clipId),
]);
writeFileSync(
  join(bgDir, "index.json"),
  JSON.stringify([{ id: clipId, mood: "any", source: "q:1", bytes: 1 }]),
);
await page.evaluate(() => window.dispatchEvent(new Event("sirtone-videos-refresh")));
await invoke("settings_patch", { patch: { viz: { mode: "video" } } });
const vid = page.locator('video[data-testid="viz-video"]');
await vid.waitFor({ timeout: 10000 }).catch(() => {});
await sleep(3000);
console.log(
  "video",
  JSON.stringify(
    await vid
      .evaluate(async (v) => {
        const t0 = v.currentTime;
        await new Promise((r) => setTimeout(r, 1500));
        return {
          t0,
          t1: v.currentTime,
          paused: v.paused,
          ready: v.readyState,
          net: v.networkState,
          err: v.error?.code ?? 0,
          dur: v.duration,
          hidden: document.hidden,
        };
      })
      .catch((e) => String(e)),
  ),
);
console.log(
  log
    .filter((l) => !l.startsWith("debug"))
    .slice(-30)
    .join("\n"),
);
app.kill();
process.exit(0);
