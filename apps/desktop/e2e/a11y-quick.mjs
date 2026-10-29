// Accessibility in the real app (T7.5, WCAG AA), isolated: axe-core injected into the main
// window — the wave screen over the live visualiser — and into the desktop widget. Critical and
// serious violations fail; text over the canvas is what axe cannot measure («incomplete»), it is
// listed for a look.
// SMOKE_PORT=9369 node apps/desktop/e2e/a11y-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
// axe-core comes with @axe-core/playwright (a dev dependency); resolved through it.
const axePw = createRequire(require.resolve("@axe-core/playwright"));
const axeSource = readFileSync(axePw.resolve("axe-core/axe.min.js"), "utf8");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-a11yq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9369);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const dir = join(dataDir, "in", "Artist", "Album");
mkdirSync(dir, { recursive: true });
for (const n of [1, 2])
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    `sine=frequency=${300 + n * 40}:duration=60`,
    "-af",
    "volume=-40dB",
    "-c:a",
    "libmp3lame",
    "-b:a",
    "32k",
    "-metadata",
    `title=Song ${n}`,
    "-metadata",
    "artist=Artist",
    "-metadata",
    "album=Sky High",
    "-metadata",
    `track=${n}`,
    join(dir, `s${n}.mp3`),
  ]);
const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
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
let page = null;
for (let i = 0; i < 80 && !page; i++) {
  page = pages().find((p) => !p.url().includes("widget")) ?? null;
  if (!page) await sleep(250);
}
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
const axe = async (p) => {
  await p.evaluate(axeSource);
  await p.evaluate(() =>
    Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined))),
  );
  return p.evaluate(async () => {
    const r = await window.axe.run(document, {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"] },
    });
    return {
      bad: r.violations
        .filter((v) => v.impact === "critical" || v.impact === "serious")
        .map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(" | ")}`),
      incomplete: r.incomplete.map((v) => `${v.id} ×${v.nodes.length}`),
    };
  });
};
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("import_paths", { paths: [join(dataDir, "in", "Artist")] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  await invoke("wave_start", { profile: null, query: null });
  await sleep(1500);
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  await sleep(3000);
  const wave = await axe(page);
  check(
    "the wave screen over the visualiser: no critical/serious",
    wave.bad.length === 0,
    wave.bad.join("; "),
  );
  console.log(
    "      to look at by eye (axe cannot measure):",
    wave.incomplete.join(", ") || "none",
  );
  await invoke("settings_patch", { patch: { widget: { enabled: true } } });
  let w = null;
  for (let i = 0; i < 40 && !w; i++) {
    w = pages().find((p) => p.url().includes("widget")) ?? null;
    if (!w) await sleep(250);
  }
  await w.waitForSelector(".w-title");
  await sleep(800);
  const widget = await axe(w);
  check("the desktop widget: no critical/serious", widget.bad.length === 0, widget.bad.join("; "));
  await invoke("settings_patch", { patch: { widget: { enabled: false } } });
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await sleep(2000);
app.kill();
console.log(failed ? "a11y-quick: FAILED" : "a11y-quick: ok");
process.exit(failed ? 1 : 0);
