// Soak of the real app (T7.2, P-13 ≤ 1 underrun in 24 h, no leaks), isolated data dir, very quiet
// but not silent. «Моя волна» with smart automix plays the whole time; every hour: a small import,
// a trip to the tray long enough for the window to be destroyed (60 s) and built again, and the
// output reopened on the Windows default by its id and back (this app's own setting only — the
// system's devices are never switched). Every 10 min one line: underruns, state, tracks played,
// private memory of the app with its WebView. Nothing else may build or run heavy work meanwhile.
// SOAK_HOURS=24 SMOKE_PORT=9373 node apps/desktop/e2e/soak-app.mjs   (log: target/soak-app/)

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { appendFileSync, mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-soak-"));
const outDir = join(root, "target", "soak-app");
mkdirSync(outDir, { recursive: true });
const logFile = join(outDir, `soak-${new Date().toISOString().replace(/[:.]/g, "-")}.log`);
const PORT = Number(process.env.SMOKE_PORT ?? 9373);
const HOURS = Number(process.env.SOAK_HOURS ?? 24);
const TICK_MS = 10 * 60 * 1000;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (line) => {
  const s = `${new Date().toISOString()}  ${line}`;
  console.log(s);
  appendFileSync(logFile, `${s}\n`);
};
// Short, varied tones: an album per batch, so the wave and automix have something to choose.
let made = 0;
const batch = (n) => {
  const dir = join(dataDir, "in", `Batch ${made}`);
  mkdirSync(dir, { recursive: true });
  for (let i = 0; i < n; i++, made++)
    spawnSync("ffmpeg", [
      "-y",
      "-v",
      "error",
      "-f",
      "lavfi",
      "-i",
      `sine=frequency=${180 + ((made * 37) % 500)}:duration=${120 + ((made * 17) % 90)}`,
      "-af",
      "volume=-30dB",
      "-c:a",
      "libmp3lame",
      "-b:a",
      "64k",
      "-metadata",
      `title=Soak ${made}`,
      "-metadata",
      `artist=Soak Artist ${made % 5}`,
      "-metadata",
      `album=Soak Album ${Math.floor(made / 6)}`,
      join(dir, `soak-${made}.mp3`),
    ]);
  return dir;
};
const first = batch(24);
const env = {
  ...process.env,
  SIRTONE_DATA_DIR: dataDir,
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
};
const app = spawn(exe, [], { env, stdio: "ignore" });
const ps = (script) =>
  execFileSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).trim();
const memMb = () =>
  Number(
    ps(
      `$all = Get-CimInstance Win32_Process; $ids = @(${app.pid}); do { $n = $ids.Count; $ids = @($ids + ($all | Where-Object { $ids -contains $_.ParentProcessId } | ForEach-Object ProcessId)) | Select-Object -Unique } while ($ids.Count -gt $n); [math]::Round(((Get-Process -Id $ids -ErrorAction SilentlyContinue | Measure-Object PrivateMemorySize64 -Sum).Sum) / 1MB)`,
    ),
  );
let browser = null;
const connect = async () => {
  await browser?.close().catch(() => {});
  for (let i = 0; i < 120; i++) {
    try {
      const b = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      browser = b;
      const p = b
        .contexts()
        .flatMap((c) => c.pages())
        .find((x) => !x.url().includes("widget"));
      if (p) return p;
    } catch {
      await sleep(250);
    }
  }
  throw new Error("no page");
};
let page = await connect();
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
const snap = () => invoke("playback_snapshot", {});
let failed = false;
let restarts = 0;
const titles = new Set();
const wave = async () => {
  await invoke("wave_start", { profile: null, query: null });
  await sleep(3000);
};
try {
  await invoke("settings_patch", {
    patch: {
      onboarding: { done: true, tips_seen: true },
      playback: { automix: { mode: "smart" } },
    },
  });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("import_paths", { paths: [first] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 120000 });
  await invoke("playback_set_volume", { volume: 0.05 });
  await wave();
  const devices = await invoke("audio_devices", {});
  const def = devices.find((d) => d.isDefault);
  const start = Date.now();
  const u0 = (await snap()).underruns;
  const m0 = memMb();
  log(`start: ${HOURS} h, underruns ${u0}, memory ${m0} MB, output «${def?.name ?? "?"}»`);
  let nextHour = start + 3600_000;
  let peak = m0;
  // The leak signal: the core alone in the tray (the WebView destroyed), hour by hour.
  const tray = [];
  while (Date.now() - start < HOURS * 3600_000) {
    await sleep(TICK_MS);
    // A full hour behind us: the hourly chores.
    if (Date.now() >= nextHour) {
      nextHour += 3600_000;
      const dir = batch(2);
      await invoke("import_paths", { paths: [dir] });
      await page
        .getByRole("button", { name: "Продолжить" })
        .click({ timeout: 10000 })
        .catch(() => {});
      ps(`(Get-Process -Id ${app.pid}).CloseMainWindow() | Out-Null`);
      await sleep(90_000);
      const trayMb = memMb();
      tray.push(trayMb);
      spawn(exe, [], { env, stdio: "ignore" });
      page = await connect();
      await page.waitForSelector("footer", { timeout: 20000 });
      if (def) {
        await invoke("settings_patch", { patch: { playback: { output: { device: def.id } } } });
        await sleep(3000);
        await invoke("settings_patch", { patch: { playback: { output: { device: "default" } } } });
      }
      log(`hourly: +2 tracks, tray ${trayMb} MB, window rebuilt, output reopened`);
    }
    const s = await snap();
    if (s.title) titles.add(s.title);
    if (s.state !== "playing") {
      restarts++;
      log(`!! not playing (${s.state}) — the wave is started again`);
      await wave();
    }
    const mb = memMb();
    peak = Math.max(peak, mb);
    log(
      `${((Date.now() - start) / 3600_000).toFixed(2)} h: underruns ${s.underruns - u0}, ${s.state}, «${s.title}», tracks seen ${titles.size}, memory ${mb} MB`,
    );
  }
  const s = await snap();
  const mEnd = memMb();
  const under = s.underruns - u0;
  log(
    `end: underruns ${under} (P-13 ≤ 1), stops ${restarts}, memory ${m0} → ${mEnd} MB (peak ${peak}), tracks seen ${titles.size}`,
  );
  // No leak: the tray memory of the last hour within 20 MB of the first.
  const grew = tray.length >= 2 ? (tray.at(-1) ?? 0) - (tray[0] ?? 0) : 0;
  log(`tray memory by hour: ${tray.join(", ")} MB (growth ${grew} MB, limit 20)`);
  failed = under > 1 || restarts > 0 || grew > 20;
  await invoke("playback_toggle", {});
} catch (e) {
  failed = true;
  log(`FAIL ${e}`);
}
await sleep(2000);
app.kill();
log(failed ? "soak-app: FAILED" : "soak-app: ok");
process.exit(failed ? 1 : 0);
