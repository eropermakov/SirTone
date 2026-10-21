// Desktop widget (T5.1, T5.18) and Bluetooth (T5.4, dry run) in the real app, isolated:
// «Показывать виджет» opens a frameless window without a taskbar button that stays under other
// windows; it shows the playing track and its ⏯ pauses; «Сон 30 мин» starts the timer; the
// place is remembered; switched off — the window goes. Bluetooth: the paired devices are listed
// and a «Подключить» is only logged (SIRTONE_SMOKE_BT_DRY) — nothing reaches a real device.
// SMOKE_PORT=9366 node apps/desktop/e2e/widget-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-widq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9366);
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
  "sine=frequency=330:duration=90",
  "-af",
  "volume=-40dB",
  "-c:a",
  "libmp3lame",
  "-b:a",
  "32k",
  "-metadata",
  "title=Widget Tone",
  tone,
]);
const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
    SIRTONE_SMOKE_BT_DRY: "1",
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
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
const pages = () => browser.contexts().flatMap((c) => c.pages());
const main = () => pages().find((p) => !p.url().includes("widget"));
const widget = () => pages().find((p) => p.url().includes("widget"));
let page = null;
for (let i = 0; i < 80 && !page; i++) {
  page = main() ?? null;
  if (!page) await sleep(250);
}
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await invoke("playback_open", { paths: [tone] });
  await sleep(1500);
  await invoke("settings_patch", { patch: { widget: { enabled: true } } });
  let w = null;
  for (let i = 0; i < 40 && !w; i++) {
    w = widget() ?? null;
    if (!w) await sleep(250);
  }
  await w.waitForSelector(".w-title");
  await sleep(800);
  const shown = await w.evaluate(() => ({
    title: document.querySelector(".w-title")?.textContent,
    bg: getComputedStyle(document.body).backgroundColor,
  }));
  check(
    "the widget shows the playing track over a transparent page",
    shown.title === (await invoke("playback_snapshot", {})).title &&
      shown.bg === "rgba(0, 0, 0, 0)",
    JSON.stringify(shown),
  );
  await w.click(".w-main");
  await sleep(800);
  const s1 = await invoke("playback_snapshot", {});
  check("⏯ in the widget pauses", s1.state === "paused", s1.state);
  await w.click(".w-sleep");
  await sleep(800);
  const sl = await invoke("sleep_status", {});
  const label = await w.evaluate(() => document.querySelector(".w-sleep")?.textContent);
  check(
    "«Сон 30 мин» starts the timer, the widget shows the time left",
    sl.phase !== "off" && /Сон: 30 мин|Сон: 29 мин/.test(label ?? ""),
    `${sl.phase} «${label}»`,
  );
  await invoke("sleep_cancel", {});
  // The widget window (found by its size; DPI-aware): not on top; moved to 200,300.
  const script = `$script:o = @(); Add-Type -Name M -Namespace U -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(System.IntPtr c); [DllImport("user32.dll")] public static extern int GetWindowLong(System.IntPtr h, int i); [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, System.IntPtr l); public delegate bool EnumProc(System.IntPtr h, System.IntPtr l); [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(System.IntPtr h, out int p); [DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr h); [DllImport("user32.dll")] public static extern bool GetWindowRect(System.IntPtr h, out RECT r); [DllImport("user32.dll")] public static extern bool SetWindowPos(System.IntPtr h, System.IntPtr a, int x, int y, int cx, int cy, uint f); public struct RECT { public int L,T,R,B; }'; [void][U.M]::SetProcessDpiAwarenessContext([System.IntPtr](-4)); [U.M]::EnumWindows({ param($h, $l) $p = 0; [void][U.M]::GetWindowThreadProcessId($h, [ref]$p); if ($p -eq PID -and [U.M]::IsWindowVisible($h)) { $q = New-Object U.M+RECT; [void][U.M]::GetWindowRect($h, [ref]$q); $w = $q.R - $q.L; $hh = $q.B - $q.T; if ($w -gt 100 -and $w -lt 1500 -and $hh -lt 500) { $script:o += ('{0:X8}' -f [U.M]::GetWindowLong($h, -20)); MOVE } }; $true }, [System.IntPtr]::Zero) | Out-Null; $script:o -join ','`;
  const ex = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      script
        .replace("PID", String(app.pid))
        .replace(
          "MOVE",
          "[void][U.M]::SetWindowPos($h, [System.IntPtr]::Zero, 200, 300, 0, 0, 0x0015)",
        ),
    ],
    { encoding: "utf8" },
  ).stdout.trim();
  const styles = ex
    .split(",")
    .filter(Boolean)
    .map((h) => Number.parseInt(h, 16));
  check(
    "the widget window is found and stays under other windows (not topmost)",
    styles.length === 1 && (styles[0] & 0x8) === 0,
    ex,
  );
  await sleep(1500);
  const doc = await invoke("settings_get", {});
  check(
    "the place is remembered",
    doc.widget.x === 200 && doc.widget.y === 300,
    `${doc.widget.x},${doc.widget.y}`,
  );
  await invoke("settings_patch", { patch: { widget: { enabled: false } } });
  await sleep(1500);
  check("switched off — the window goes", !widget(), String(pages().length));
  // Bluetooth, dry run.
  const bt = await invoke("bt_devices", {});
  check(
    "Bluetooth: the paired audio devices are listed",
    Array.isArray(bt),
    bt.map((d) => `${d.name}${d.connected ? " *" : ""}`).join(", ") || "none paired",
  );
  const name = bt[0]?.name ?? "Test device";
  const ok = await invoke("bt_set", { name, connect: !bt[0]?.connected });
  await sleep(500);
  const logDir = join(dataDir, "logs");
  const logs = readdirSync(logDir)
    .map((f) => readFileSync(join(logDir, f), "utf8"))
    .join("\n");
  check(
    "«Подключить» in a test is a dry run — only logged, no device touched",
    ok === true && logs.includes("dry run — nothing sent"),
  );
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await sleep(2500);
app.kill();
console.log(failed ? "widget-quick: FAILED" : "widget-quick: ok");
process.exit(failed ? 1 : 0);
