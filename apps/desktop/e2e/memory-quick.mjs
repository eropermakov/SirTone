// Memory in the tray (T5.10, FR-SHELL-009) in the real app, isolated: playing, the window is
// closed to the tray (WM_CLOSE to this test's own window only); the WebView is suspended and —
// after the test delay (SIRTONE_SMOKE_TRAY_MS instead of 60 s) — destroyed: the app's processes
// fall to ≤ 120 MB private memory while the music plays on; a second start (single instance)
// builds the window again and it shows the same track further on.
// SMOKE_PORT=9365 node apps/desktop/e2e/memory-quick.mjs

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { closeMainScript } from "./close-main.mjs";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-memq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9365);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const dir = join(dataDir, "in", "Artist", "Album");
mkdirSync(dir, { recursive: true });
const tone = join(dir, "t.mp3");
spawnSync("ffmpeg", [
  "-y",
  "-v",
  "error",
  "-f",
  "lavfi",
  "-i",
  "sine=frequency=330:duration=120",
  "-af",
  "volume=-40dB",
  "-c:a",
  "libmp3lame",
  "-b:a",
  "32k",
  "-metadata",
  "title=Memory Tone",
  tone,
]);
const env = {
  ...process.env,
  SIRTONE_DATA_DIR: dataDir,
  SIRTONE_SMOKE_TRAY_MS: "4000",
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
};
const app = spawn(exe, [], { env, stdio: "ignore" });
const connect = async () => {
  for (let i = 0; i < 80; i++) {
    try {
      const b = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      const p = b.contexts()[0]?.pages()[0];
      if (p) return p;
    } catch {
      await sleep(250);
    }
  }
  return null;
};
const ps = (script) =>
  execFileSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).trim();
// WM_CLOSE to this test's own main window (D-124: not Process.CloseMainWindow).
const closeMain = () => {
  if (ps(closeMainScript(app.pid)) !== "closed") throw new Error("no main window to close");
};
// Private memory (MB) of the app and every process it started (WebView2).
const memMb = () =>
  Number(
    ps(
      `$all = Get-CimInstance Win32_Process; $ids = @(${app.pid}); do { $n = $ids.Count; $ids = @($ids + ($all | Where-Object { $ids -contains $_.ParentProcessId } | ForEach-Object ProcessId)) | Select-Object -Unique } while ($ids.Count -gt $n); [math]::Round(((Get-Process -Id $ids -ErrorAction SilentlyContinue | Measure-Object PrivateMemorySize64 -Sum).Sum) / 1MB)`,
    ),
  );
// Titles of the system media sessions (SMTC) — Windows' own list, read only.
const smtcTitles = () =>
  ps(
    `Add-Type -AssemblyName System.Runtime.WindowsRuntime; $as = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation\`1' })[0]; function Await($op, $t) { $task = $as.MakeGenericMethod($t).Invoke($null, @($op)); $task.Wait(5000) | Out-Null; $task.Result }; [void][Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType = WindowsRuntime]; $m = Await ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]); ($m.GetSessions() | ForEach-Object { (Await ($_.TryGetMediaPropertiesAsync()) ([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties])).Title }) -join '|'`,
  );
const frames = () =>
  page.evaluate(
    () =>
      new Promise((resolve) => {
        let n = 0;
        const t0 = performance.now();
        const step = () => {
          n += 1;
          if (performance.now() - t0 < 600) requestAnimationFrame(step);
          else resolve(n);
        };
        requestAnimationFrame(step);
      }),
  );
const relaunch = async () => {
  spawn(exe, [], { env, stdio: "ignore" });
  await sleep(2500);
};
const webviews = () =>
  Number(
    ps(
      `(Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.ParentProcessId -eq ${app.pid} }).Count`,
    ),
  );
let page = await connect();
await page.waitForSelector("footer");
let invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await invoke("playback_open", { paths: [tone] });
  await sleep(3000);
  const playingTitle = (await invoke("playback_snapshot", {})).title;
  // A short trip to the tray (before the window is destroyed): the page wakes up again.
  closeMain();
  await sleep(1500);
  await relaunch();
  const f = await frames();
  const footer = await page.locator("footer").innerText();
  check(
    "hidden and opened again within the delay: the page is awake (frames run, player shown)",
    f > 10 && !!playingTitle && footer.includes(playingTitle),
    `${f} frames in 0.6 s`,
  );
  const shownMb = memMb();
  const s0 = await invoke("playback_snapshot", {});
  // Close to the tray: WM_CLOSE to this test's own process window only.
  closeMain();
  await sleep(8000);
  const trayMb = memMb();
  const wv = webviews();
  check(
    "in the tray the window and its WebView are gone: ≤ 120 MB, music on",
    trayMb <= 120 && wv === 0,
    `shown ${shownMb} MB → tray ${trayMb} MB, webview processes ${wv}`,
  );
  const titles = smtcTitles();
  check(
    "without the window the media keys and the Windows flyout still have Сиртон (SMTC, AT-03)",
    titles.split("|").includes(playingTitle),
    titles,
  );
  // Second start → single instance → the window is built again.
  spawn(exe, [], { env, stdio: "ignore" });
  await sleep(1000);
  page = await connect();
  await page.waitForSelector("footer", { timeout: 15000 });
  invoke = (cmd, args) =>
    page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
  const s1 = await invoke("playback_snapshot", {});
  check(
    "opened again: the same track, further on, still playing",
    s1.state === "playing" && s1.title === s0.title && s1.positionMs > s0.positionMs + 8000,
    `${s0.positionMs} ${s0.state} → ${s1.positionMs} ${s1.state}`,
  );
  await invoke("playback_toggle", {});
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await sleep(2500);
app.kill();
console.log(failed ? "memory-quick: FAILED" : "memory-quick: ok");
process.exit(failed ? 1 : 0);
