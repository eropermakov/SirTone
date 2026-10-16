// Real-app smoke on the built exe (release checklist). Not part of `pnpm e2e`: it drives the real
// app over WebView2 DevTools. The app runs ISOLATED (SIRTONE_DATA_DIR = fresh temp dir → own
// settings, logs, WebView2 profile and single-instance identity): the owner's running Сиртон and
// data are never touched, and nothing has to be closed first.
//
//   node apps/desktop/e2e/real-app.mjs [path\to\sirtone-app.exe]
//   (default exe: target\release\sirtone-app.exe; installed: %LOCALAPPDATA%\SirTone\sirtone-app.exe)
//
// Checks: command-line file plays (+ audio via loopback when the S-01 tool is built), Space,
// seek, broken file → toast, folder → first file, second launch forwards a file, theme and
// volume persist across a restart.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");

const root = join(import.meta.dirname, "..", "..", "..");
const exe = process.argv[2] ?? join(root, "target", "release", "sirtone-app.exe");
const fixtures = join(root, "fixtures", "generated");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-smoke-"));
const PORT = 9335;
const env = {
  ...process.env,
  SIRTONE_DATA_DIR: dataDir,
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
};

const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const toMs = (tc) => {
  const p = tc.split(":").map(Number);
  return (p.length === 3 ? p[0] * 3600 + p[1] * 60 + p[2] : p[0] * 60 + p[1]) * 1000;
};
const settings = () => JSON.parse(readFileSync(join(dataDir, "settings.json"), "utf8"));

function launch(args = []) {
  return spawn(exe, args, { env, detached: false, stdio: "ignore" });
}
async function connect() {
  for (let i = 0; i < 60; i++) {
    try {
      const b = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      const page = b.contexts()[0]?.pages()[0];
      if (page) {
        await page.waitForSelector("footer");
        return { browser: b, page };
      }
      await b.close();
    } catch {
      // not up yet
    }
    await sleep(250);
  }
  throw new Error("app did not expose DevTools");
}

let app = launch([join(fixtures, "long", "mix-3h.mp3")]);
let { browser, page } = await connect();
let footer = page.locator("footer");
const times = () => footer.locator(".t-timecode").allTextContents();
const playLabel = () => footer.locator("button[aria-label]").nth(2).getAttribute("aria-label");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);

// 1. Command-line file plays — and is audible (process loopback, S-01 tool).
await sleep(1200);
const title = await footer.locator(".t-body-strong").first().textContent();
check("title from file name", title === "mix-3h", title);
const [a] = await times();
await sleep(1500);
const [b, total] = await times();
check("position advances while playing", toMs(b) > toMs(a), `${a} → ${b} of ${total}`);
check("play button shows pause", (await playLabel()) === "Пауза");
const loopback = join(root, "target", "spikes", "release", "s01-output.exe");
if (existsSync(loopback)) {
  const out = spawnSync(loopback, [
    "--capture-pid",
    String(app.pid),
    "--seconds",
    "2",
  ]).stdout.toString();
  const m = out.match(
    /signal ([\d.]+) s, RMS (-?[\d.]+) dBFS, 10 ms silent blocks inside signal: (\d+), discontinuity flags: (\d+)/,
  );
  check(
    "audible without gaps (loopback)",
    m && Number(m[1]) > 1.8 && Number(m[2]) > -80 && m[3] === "0" && m[4] === "0",
    m ? m[0] : out.trim(),
  );
}

// 2. Space toggles.
await page.evaluate(() => document.activeElement?.blur());
await page.keyboard.press("Space");
await sleep(400);
check("space pauses", (await playLabel()) === "Слушать");
const [p1] = await times();
await sleep(1200);
const [p2] = await times();
check("position frozen while paused", p1 === p2, `${p1} / ${p2}`);
await page.keyboard.press("Space");
await sleep(400);
check("space resumes", (await playLabel()) === "Пауза");

// 3. Seek to the middle by clicking the progress bar.
const bar = footer.getByRole("slider", { name: "Позиция в треке" });
const box = await bar.boundingBox();
await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
await sleep(600);
const [s, dur] = await times();
check("seek to middle", Math.abs(toMs(s) / toMs(dur) - 0.5) < 0.02, `${s} of ${dur}`);

// 4. Broken file → toast, current track keeps playing.
await invoke("playback_open", { paths: [join(fixtures, "broken", "text-as.mp3")] });
const toast = page.getByRole("status");
await toast.waitFor({ timeout: 3000 }).catch(() => {});
const toastText = (await toast.count()) ? await toast.textContent() : "";
check(
  "error toast for broken file",
  toastText.includes("не поддерживается") && toastText.includes("text-as.mp3"),
  toastText,
);
check("still playing after error", (await playLabel()) === "Пауза");

// 5. Space still toggles after the volume slider was touched (no blur).
await footer.getByRole("slider", { name: "Громкость" }).click();
await page.keyboard.press("Space");
await sleep(400);
check("space works after slider focus", (await playLabel()) === "Слушать");
await page.keyboard.press("Space");
await sleep(400);

// 6. A folder plays its first playable file.
await invoke("playback_open", { paths: [join(fixtures, "formats")] });
await sleep(800);
const folderTitle = await footer.locator(".t-body-strong").first().textContent();
check("folder plays first file", folderTitle === "aac-256", folderTitle);

// 7. Second launch with a file («Открыть с помощью» while running) → forwarded, plays.
launch([join(fixtures, "formats", "flac-16-44.flac")]);
await sleep(2000);
const t2 = await footer.locator(".t-body-strong").first().textContent();
check("second launch forwards the file", t2 === "flac-16-44", t2);

// 8. Theme and volume persist through Rust settings (T0.6/T0.7, D-64) and survive a restart.
await page.getByRole("button", { name: "Настройки" }).click();
await page.getByRole("button", { name: "Светлая" }).click();
await invoke("playback_set_volume", { volume: 0.37 });
await sleep(1600); // debounced write ≤ 1 s
check("theme saved", settings().appearance.theme === "light");
check("volume saved", Math.abs(settings().playback.volume - 0.37) < 1e-6);
await browser.close();
app.kill();
await sleep(800);
app = launch();
({ browser, page } = await connect());
footer = page.locator("footer");
await sleep(800);
check("theme after restart", (await page.locator("html").getAttribute("data-theme")) === "light");
const vol = await footer.getByRole("slider", { name: "Громкость" }).getAttribute("aria-valuenow");
check("volume after restart", vol === "37", vol);

await browser.close();
app.kill();
await sleep(500);
rmSync(dataDir, { recursive: true, force: true });
const failed = results.filter((r) => !r.ok).length;
console.log(
  `\n${results.length - failed}/${results.length} passed (isolated data dir, owner's app untouched)`,
);
process.exit(failed ? 1 : 0);
