// Loudness normalization end to end (T4.11, FR-PLAY-008): an isolated app, a steady tone in
// the library, its loudness measured by the analysis (B0); the loopback RMS of what the device
// plays with normalization off and on must differ by the computed gain (±0.2 dB).
// Needs target/spikes/release/s01-output.exe (loopback capture by pid) and the built app.
// SMOKE_PORT=9349 node apps/desktop/e2e/norm-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const loopback = join(root, "target", "spikes", "release", "s01-output.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-normq-"));
const libDir = join(dataDir, "Music", "SirTone");
const PORT = Number(process.env.SMOKE_PORT ?? 9349);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
if (!existsSync(loopback)) {
  console.log("SKIP  no loopback tool (s01-output.exe)");
  process.exit(0);
}
const src = join(dataDir, "in", "Tone");
mkdirSync(src, { recursive: true });
copyFileSync(join(root, "fixtures", "generated", "signals", "tone-10s.wav"), join(src, "tone.wav"));

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
  await invoke("import_paths", { paths: [src] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  // The analysis starts 20 s after launch; the tone is short.
  const db = () => new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
  let row;
  for (let i = 0; i < 240 && !row?.lufs_i; i++) {
    await sleep(1000);
    if (i % 20 === 19) console.log("analysis", JSON.stringify(await invoke("analysis_progress", {})));
    const d = db();
    row = d.prepare("SELECT id, lufs_i, true_peak_db FROM track LIMIT 1").get();
    d.close();
  }
  check("the tone is measured (B0)", row?.lufs_i != null, JSON.stringify(row));
  // gain_db(-14, L, TP): clamp(−14 − L, −12, +6), ≤ −1 − TP.
  const want = Math.min(Math.max(-14 - row.lufs_i, -12), 6, -1 - row.true_peak_db);
  const rms = () => {
    const out = spawnSync(loopback, [
      "--capture-pid",
      String(app.pid),
      "--seconds",
      "2",
    ]).stdout.toString();
    const m = out.match(/RMS (-?[\d.]+) dBFS/);
    return m ? Number(m[1]) : null;
  };
  await invoke("playback_set_volume", { volume: 0.5 });
  const norm = (enabled) =>
    invoke("settings_patch", {
      patch: { playback: { normalization: { enabled, target_lufs: -14 } } },
    });
  await norm(false);
  await invoke("play_tracks", { ids: [row.id], start: 0, context: "collection", playlist: null });
  await sleep(1500);
  const off = rms();
  await norm(true);
  await sleep(800);
  const on = rms();
  check(
    "normalization on: the level moves by the computed gain (±0.2 dB), live",
    off !== null && on !== null && Math.abs(on - off - want) <= 0.2,
    `off ${off} dBFS, on ${on} dBFS, Δ ${(on - off).toFixed(2)} dB, want ${want.toFixed(2)} dB (L ${row.lufs_i.toFixed(2)}, TP ${row.true_peak_db.toFixed(2)})`,
  );
  // «Тихо» (−19): 5 dB less than «Обычно».
  await invoke("settings_patch", { patch: { playback: { normalization: { target_lufs: -19 } } } });
  await sleep(800);
  const quiet = rms();
  check(
    "«Тихо» is 5 dB under «Обычно»",
    quiet !== null && Math.abs(on - quiet - 5) <= 0.2,
    `${quiet} dBFS`,
  );
} catch (e) {
  check("run", false, String(e).slice(0, 300));
}
app.kill();
process.exit(failed ? 1 : 0);
