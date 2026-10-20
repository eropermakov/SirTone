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
    if (i % 20 === 19)
      console.log("analysis", JSON.stringify(await invoke("analysis_progress", {})));
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
  // The tone is 10 s: repeat it so every measurement hears it.
  await invoke("playback_set_repeat", { mode: "one" });
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
  // T4.12: the equalizer — −12 dB at 500 Hz pulls the 440/660 Hz tone down a lot; flat bands
  // change nothing; off restores the level.
  const eq = (enabled, bands) =>
    invoke("settings_patch", {
      patch: { playback: { eq: { enabled, preamp_db: 0, bands_db: bands } } },
    });
  const flat = Array(10).fill(0);
  await eq(true, flat);
  await sleep(800);
  const eqFlat = rms();
  const cut = [...flat];
  cut[4] = -12;
  await eq(true, cut);
  await sleep(800);
  const eqCut = rms();
  await eq(false, cut);
  await sleep(800);
  const eqOff = rms();
  check(
    "equalizer: flat = no change, −12 dB at 500 Hz cuts the tone ≥ 6 dB, off restores",
    Math.abs(eqFlat - quiet) <= 0.15 && quiet - eqCut >= 6 && Math.abs(eqOff - quiet) <= 0.2,
    `flat ${eqFlat}, cut ${eqCut}, off ${eqOff} (before ${quiet}) dBFS`,
  );
  // T4.13: the signal path — nothing on and volume 100 % → «Bit-perfect» or «ресемплинг»
  // (the 44.1 kHz tone on a 48 kHz device); normalization on → «DSP» (the limiter runs).
  await invoke("settings_patch", { patch: { playback: { normalization: { enabled: false } } } });
  await invoke("playback_set_volume", { volume: 1 });
  await sleep(800);
  const clean = (await invoke("playback_snapshot", {})).signal;
  await invoke("settings_patch", { patch: { playback: { normalization: { enabled: true } } } });
  await sleep(800);
  const processed = (await invoke("playback_snapshot", {})).signal;
  const wantClean = clean?.sourceRate === clean?.deviceRate ? "bitperfect" : "resampled";
  check(
    "signal path: clean chain → bit-perfect/resampled, normalization → dsp",
    clean?.kind === wantClean && processed?.kind === "dsp",
    `${JSON.stringify(clean)} → ${JSON.stringify(processed)}`,
  );
  await invoke("playback_set_volume", { volume: 0.5 });
} catch (e) {
  check("run", false, String(e).slice(0, 300));
}
app.kill();
process.exit(failed ? 1 : 0);
