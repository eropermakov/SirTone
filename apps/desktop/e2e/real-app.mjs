// Real-app smoke over WebView2 DevTools (release checklist, prototype 0.2+). Not part of `pnpm e2e`:
// it drives the built exe, not the Vite preview.
//
// 1. Start the app with a debugging port and a quiet audio file:
//    $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9333'
//    target\release\sirtone-app.exe fixtures\generated\long\mix-3h.mp3
// 2. node apps/desktop/e2e/real-app.mjs
//
// Checks: file from the command line plays, Space toggles, seek, error toast for a broken file,
// settings round-trip to %LOCALAPPDATA%\SirTone\settings.json. The owner's theme and volume (D-64)
// are recorded first and restored at the end. Volume across a restart: e2e/real-app-volume.mjs.
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");

const root = join(import.meta.dirname, "..", "..", "..");
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

const settingsFile = join(process.env.LOCALAPPDATA, "SirTone", "settings.json");
const readSettings = () => JSON.parse(readFileSync(settingsFile, "utf8"));
const ownerVolume = readSettings().playback?.volume ?? 1;

const browser = await chromium.connectOverCDP("http://127.0.0.1:9333");
const page = browser.contexts()[0].pages()[0];
const footer = page.locator("footer");
const times = () => footer.locator(".t-timecode").allTextContents();
const playLabel = () => footer.locator("button[aria-label]").nth(2).getAttribute("aria-label");

// 1. Command-line file plays.
const title = await footer.locator(".t-body-strong").first().textContent();
check("title from file name", title === "mix-3h", title);
const [a] = await times();
await sleep(1500);
const [b, total] = await times();
check("position advances while playing", toMs(b) > toMs(a), `${a} → ${b} of ${total}`);
check("play button shows pause", (await playLabel()) === "Пауза");

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
const frac = toMs(s) / toMs(dur);
check("seek to middle", Math.abs(frac - 0.5) < 0.02, `${s} of ${dur}`);

// 4. Broken file → toast, current track keeps playing.
await page.evaluate(
  (path) => window.__TAURI_INTERNALS__.invoke("playback_open", { paths: [path] }),
  join(root, "fixtures", "generated", "broken", "text-as.mp3"),
);
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
await page.evaluate(
  (path) => window.__TAURI_INTERNALS__.invoke("playback_open", { paths: [path] }),
  join(root, "fixtures", "generated", "formats"),
);
await sleep(800);
const folderTitle = await footer.locator(".t-body-strong").first().textContent();
check("folder plays first file", folderTitle === "aac-256", folderTitle);

// 7. Second launch with a file («Открыть с помощью» while running) → forwarded, plays.
if (process.env.SIRTONE_EXE) {
  const { spawn } = await import("node:child_process");
  spawn(
    process.env.SIRTONE_EXE,
    [join(root, "fixtures", "generated", "formats", "flac-16-44.flac")],
    {
      detached: true,
      stdio: "ignore",
    },
  ).unref();
  await sleep(2000);
  const t2 = await footer.locator(".t-body-strong").first().textContent();
  check("second launch forwards the file", t2 === "flac-16-44", t2);
}

// 8. Settings round-trip through Rust (T0.6/T0.7).
const before = JSON.parse(readFileSync(settingsFile, "utf8")).appearance.theme;
await page.getByRole("button", { name: "Настройки" }).click();
const target = before === "light" ? "Тёмная" : "Светлая";
await page.getByRole("button", { name: target }).click();
await sleep(1600); // debounced write ≤ 1 s
const after = JSON.parse(readFileSync(settingsFile, "utf8")).appearance.theme;
check(
  "theme persisted to settings.json",
  after === (before === "light" ? "dark" : "light"),
  `${before} → ${after}`,
);
const restore = { system: "Как в системе", dark: "Тёмная", light: "Светлая" }[before];
await page.getByRole("button", { name: restore }).click();
await sleep(1600);
check("theme restored", JSON.parse(readFileSync(settingsFile, "utf8")).appearance.theme === before);

// Restore the owner's volume (the slider steps above changed it and D-64 saved it).
await page.evaluate(
  (v) => window.__TAURI_INTERNALS__.invoke("playback_set_volume", { volume: v }),
  ownerVolume,
);
await sleep(1600);
check(
  "owner volume restored",
  Math.abs((readSettings().playback?.volume ?? 1) - ownerVolume) < 1e-6,
  String(ownerVolume),
);

await browser.close();
const failed = results.filter((r) => !r.ok).length;
console.log(`\n${results.length - failed}/${results.length} passed`);
process.exit(failed ? 1 : 0);
