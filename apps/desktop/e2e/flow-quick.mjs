// «Волны» only flow (owner 2026-10-20, D-107): on a 120 BPM kick track the field's shape never
// changes with the beat (warp step ≈ 0), the flow speeds up with the music (push > 0) and the
// speed-up glides (small frame-to-frame change).
// SMOKE_PORT=9359 node apps/desktop/e2e/flow-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-flowq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9359);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const dir = join(dataDir, "in", "Kick", "Album");
mkdirSync(dir, { recursive: true });
spawnSync("ffmpeg", [
  "-y",
  "-v",
  "error",
  "-f",
  "lavfi",
  "-i",
  "aevalsrc='0.8*sin(2*PI*55*t)*exp(-18*mod(t,0.5))':s=44100:d=60",
  "-ac",
  "2",
  "-c:a",
  "flac",
  "-metadata",
  "title=Kick",
  "-metadata",
  "artist=Kick",
  join(dir, "kick.flac"),
]);
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
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("import_paths", { paths: [join(dataDir, "in", "Kick")] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
  await invoke("wave_start", {});
  await invoke("playback_set_repeat", { mode: "one" });
  await sleep(2000);
  // Fresh statistics over 10 s of beats.
  await page.evaluate(() => {
    const s = window.__sirtoneWaves.stats;
    s.warpStep = 0;
    s.pushStep = 0;
    s.pushMax = 0;
  });
  await sleep(10000);
  const s = await page.evaluate(() => ({ ...window.__sirtoneWaves.stats, samples: undefined }));
  check(
    "the shape never follows the beat",
    s.warpStep < 0.005,
    `warp step ${s.warpStep.toFixed(4)}`,
  );
  check(
    "the music speeds the flow up",
    s.pushMax > 0.2,
    `peak speed-up ×${(1 + s.pushMax).toFixed(2)}`,
  );
  check(
    "the speed-up glides (≤ 0.06 per frame)",
    s.pushStep <= 0.06,
    `largest step ${s.pushStep.toFixed(3)} at ${s.fps.toFixed(0)} FPS`,
  );
} catch (e) {
  check("run", false, String(e).slice(0, 400));
}
app.kill();
process.exit(failed ? 1 : 0);
