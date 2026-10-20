// UI sounds (T4.15, FR-PLAY-016): an isolated app with no music; UI sounds triggered every
// 200 ms are heard in Sirtone's own loopback (through the engine, no WebView audio session);
// switched off — silence.
// SMOKE_PORT=9352 node apps/desktop/e2e/sfx-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const loopback = join(root, "target", "spikes", "release", "s01-output.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-sfxq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9352);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
if (!existsSync(loopback)) {
  console.log("SKIP  no loopback tool");
  process.exit(0);
}
const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
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
const listen = async () => {
  await page.evaluate(() => {
    window.__sfxTimer = setInterval(
      () => window.__TAURI_INTERNALS__.invoke("ui_sound", { name: "import_done" }),
      200,
    );
  });
  const out = spawnSync(loopback, [
    "--capture-pid",
    String(app.pid),
    "--seconds",
    "2",
  ]).stdout.toString();
  await page.evaluate(() => clearInterval(window.__sfxTimer));
  const m = out.match(/signal ([\d.]+) s, RMS (-?[\d.]+) dBFS/);
  return m
    ? { secs: Number(m[1]), rms: Number(m[2]) }
    : { secs: 0, rms: -999, raw: out.trim().slice(0, 120) };
};
try {
  await invoke("settings_patch", { patch: { sfx: { enabled: true, volume: 0.5 } } });
  await sleep(500);
  const on = await listen();
  check(
    "UI sounds play through the engine (no music loaded)",
    on.secs > 0.5 && on.rms > -60,
    JSON.stringify(on),
  );
  await invoke("settings_patch", { patch: { sfx: { enabled: false } } });
  await sleep(500);
  const off = await listen();
  check("switched off — silence", off.secs < 0.05, JSON.stringify(off));
} catch (e) {
  check("run", false, String(e).slice(0, 300));
}
app.kill();
process.exit(failed ? 1 : 0);
