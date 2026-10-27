// Soak of the real app (T7.2, P-13 ≤ 1 underrun in 24 h, no leaks), isolated data dir, very quiet
// but not silent. «Моя волна» with smart automix plays the whole time on a small library (24
// tracks ≈ 1 h, +2 an hour) — so the repeat rule bends (D-119) many times over. The first
// SHOWN_HOURS the window stays shown and the WebView must not grow; after that, every hour: a
// small import, a trip to the tray long enough for the window to be destroyed (60 s) and built
// again, and the output reopened on the Windows default by its id and back (this app's own
// setting only). DevTools (CDP) is attached only for each check and detached at once: attached,
// it keeps every IPC request in the renderer and the memory numbers would be its own (T7.2).
// The first stop of the music fails the run — except the app's own pause when Windows takes the
// output away (a Bluetooth device or driver event, FR: pause rather than jump to another device):
// that is logged and played on only when the default output is the same one the run started on,
// so the soak never plays into someone's headphones. Nothing else may build or run heavy work meanwhile.
// SOAK_HOURS=24 SMOKE_PORT=9373 node apps/desktop/e2e/soak-app.mjs   (log: target/soak-app/)

import { execFileSync, spawn, spawnSync } from "node:child_process";
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { closeMainScript } from "./close-main.mjs";

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
// A dry run shortens the hour and the tick (SOAK_HOUR_MIN=3 SOAK_TICK_MIN=1).
const HOUR_MS = Number(process.env.SOAK_HOUR_MIN ?? 60) * 60 * 1000;
const TICK_MS = Number(process.env.SOAK_TICK_MIN ?? 10) * 60 * 1000;
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
const SHOWN_HOURS = Number(process.env.SOAK_SHOWN_HOURS ?? 6);
const ps = (script) =>
  execFileSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).trim();
// WM_CLOSE to this test's own main window (D-124: not Process.CloseMainWindow).
const closeMain = () => {
  if (ps(closeMainScript(app.pid)) !== "closed") throw new Error("no main window to close");
};
// Private memory (MB): the core process and its WebView processes apart.
const mem = () => {
  const [core, web] = ps(
    `$all = Get-CimInstance Win32_Process; $ids = @(${app.pid}); do { $n = $ids.Count; $ids = @($ids + ($all | Where-Object { $ids -contains $_.ParentProcessId } | ForEach-Object ProcessId)) | Select-Object -Unique } while ($ids.Count -gt $n); $c = (Get-Process -Id ${app.pid}).PrivateMemorySize64; $wid = @($ids | Where-Object { $_ -ne ${app.pid} }); $w = if ($wid.Count) { ((Get-Process -Id $wid -ErrorAction SilentlyContinue | Measure-Object PrivateMemorySize64 -Sum).Sum) } else { 0 }; "$([math]::Round($c / 1MB)) $([math]::Round($w / 1MB))"`,
  )
    .split(" ")
    .map(Number);
  return { core, web, text: `core ${core} + webview ${web} MB` };
};
// The WebView by process kind (browser, renderer, gpu-process, utility): the sum alone moves with
// the GPU process and cannot tell a leak from a cache.
const memSplit = () =>
  ps(
    `$all = Get-CimInstance Win32_Process; $ids = @(${app.pid}); do { $n = $ids.Count; $ids = @($ids + ($all | Where-Object { $ids -contains $_.ParentProcessId } | ForEach-Object ProcessId)) | Select-Object -Unique } while ($ids.Count -gt $n); $all | Where-Object { $ids -contains $_.ProcessId -and $_.ProcessId -ne ${app.pid} } | ForEach-Object { $k = if ($_.CommandLine -match '--type=([a-z-]+)') { $Matches[1] } else { 'browser' }; [pscustomobject]@{ k = $k; mb = (Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue).PrivateMemorySize64 / 1MB } } | Group-Object k | ForEach-Object { '{0} {1:N0}' -f $_.Name, ($_.Group | Measure-Object mb -Sum).Sum } | Sort-Object`,
  )
    .split(/\r?\n/)
    .join(", ");
// Attach, do, detach.
const withPage = async (fn) => {
  for (let i = 0; i < 120; i++) {
    let browser = null;
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      const page = browser
        .contexts()
        .flatMap((c) => c.pages())
        .find((x) => !x.url().includes("widget"));
      if (page) {
        await page.waitForSelector("footer", { timeout: 20000 });
        const invoke = (cmd, args) =>
          page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
        return await fn(page, invoke);
      }
    } catch (e) {
      if (i === 119) throw e;
    } finally {
      await browser?.close().catch(() => {});
    }
    await sleep(250);
  }
  throw new Error("no page");
};
const snap = () => withPage((_, invoke) => invoke("playback_snapshot", {}));
let failed = false;
let stopped = false;
let deviceLost = 0; // the app's own pauses when Windows took the output away
// How many times the app logged «output device lost — paused» so far (all daily log files).
const lostInLog = () => {
  const dir = join(dataDir, "logs");
  if (!existsSync(dir)) return 0;
  return readdirSync(dir)
    .filter((f) => f.startsWith("sirtone."))
    .reduce(
      (n, f) =>
        n + readFileSync(join(dir, f), "utf8").split("output device lost — paused").length - 1,
      0,
    );
};
const titles = new Set();
try {
  await withPage(async (page, invoke) => {
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
    await invoke("wave_start", { profile: null, query: null });
  });
  await sleep(3000);
  const def = await withPage((_, invoke) =>
    invoke("audio_devices", {}).then((d) => d.find((x) => x.isDefault)),
  );
  const start = Date.now();
  const u0 = (await snap()).underruns;
  const m0 = mem();
  log(
    `start: ${HOURS} h (${SHOWN_HOURS} h shown first), underruns ${u0}, ${m0.text}, output «${def?.name ?? "?"}»`,
  );
  const shown = []; // webview MB at each full hour while shown
  const tray = []; // core MB in the tray, hour by hour
  let nextHour = start + HOUR_MS;
  let hour = 0;
  while (Date.now() - start < HOURS * HOUR_MS) {
    await sleep(TICK_MS);
    if (Date.now() >= nextHour) {
      nextHour += HOUR_MS;
      hour += 1;
      if (hour <= SHOWN_HOURS) {
        shown.push(mem().web);
        log(`webview by kind: ${memSplit()} MB`);
      } else {
        const dir = batch(2);
        await withPage(async (page, invoke) => {
          await invoke("import_paths", { paths: [dir] });
          await page
            .getByRole("button", { name: "Продолжить" })
            .click({ timeout: 10000 })
            .catch(() => {});
        });
        log(`before the tray: ${mem().text}`);
        closeMain();
        await sleep(90_000);
        const trayMb = mem().core;
        tray.push(trayMb);
        // A second launch hands over to the running app and exits (FR-SHELL-004, D-124).
        const again = spawn(exe, [], { env, stdio: "ignore" });
        await sleep(10_000);
        const copies = ps(
          `@(Get-CimInstance Win32_Process -Filter "Name='sirtone-app.exe'" | Where-Object { $_.ExecutablePath -eq '${exe}' }).Count`,
        ).trim();
        if (again.exitCode === null || copies !== "1") {
          if (again.exitCode === null) again.kill();
          throw new Error(`a second launch stayed up: ${copies} copies of the app`);
        }
        if (def)
          await withPage(async (_, invoke) => {
            await invoke("settings_patch", { patch: { playback: { output: { device: def.id } } } });
            await sleep(3000);
            await invoke("settings_patch", {
              patch: { playback: { output: { device: "default" } } },
            });
          });
        log(
          `hourly: +2 tracks, tray ${trayMb} MB, window rebuilt (${mem().text}), output reopened`,
        );
      }
    }
    const s = await snap();
    if (s.title) titles.add(s.title);
    log(
      `${((Date.now() - start) / HOUR_MS).toFixed(2)} h: underruns ${s.underruns - u0}, ${s.state}, «${s.title}», tracks seen ${titles.size}, ${mem().text}`,
    );
    // «Моя волна» never ends: the first stop fails the run at once (a restart would hide it).
    const lost = lostInLog();
    if (s.state === "paused" && lost > deviceLost) {
      deviceLost = lost;
      const now = await withPage((_, invoke) =>
        invoke("audio_devices", {}).then((d) => d.find((x) => x.isDefault)),
      );
      if (def && now?.id === def.id) {
        await withPage((_, invoke) => invoke("playback_toggle", {}));
        await sleep(5000);
        const r = await snap();
        log(`device event: Windows took the output away, the app paused; resumed — ${r.state}`);
        if (r.state === "playing") continue;
      } else {
        log(
          `device event: the default output is now «${now?.name ?? "?"}», not the run's — not resuming`,
        );
      }
    }
    if (s.state !== "playing") {
      stopped = true;
      log(`!! STOPPED (${s.state}) — the run ends here`);
      break;
    }
  }
  const s = await snap();
  const under = s.underruns - u0;
  log(
    `end: underruns ${under} (P-13 ≤ 1), ${mem().text}, tracks seen ${titles.size}, device events ${deviceLost}`,
  );
  // No leak while shown: the WebView at the last shown hour within 60 MB of the first.
  const grewShown = shown.length >= 2 ? (shown.at(-1) ?? 0) - (shown[0] ?? 0) : 0;
  log(`webview while shown, by hour: ${shown.join(", ")} MB (growth ${grewShown} MB, limit 60)`);
  // No leak in the core: the tray memory of the last hour within 20 MB of the first.
  const grewTray = tray.length >= 2 ? (tray.at(-1) ?? 0) - (tray[0] ?? 0) : 0;
  log(`core in the tray, by hour: ${tray.join(", ")} MB (growth ${grewTray} MB, limit 20)`);
  failed = under > 1 || stopped || grewShown > 60 || grewTray > 20;
  if (s.state === "playing") await withPage((_, invoke) => invoke("playback_toggle", {}));
} catch (e) {
  failed = true;
  log(`FAIL ${e}`);
}
await sleep(2000);
app.kill();
log(failed ? "soak-app: FAILED" : "soak-app: ok");
process.exit(failed ? 1 : 0);
