// Owner feedback 2026-10-20 (D-105) in the real app, isolated data dir:
// 1) «Волны» glide: the warp never jumps from frame to frame; 2) no live effects over MilkDrop;
// 3) 🔒 holds across a track change and a mode switch, and is saved; 4) «Только мои»: only ♥
// presets take turns; 5) «Полный экран»: title, artist and the transport, Esc leaves.
// SMOKE_PORT=9358 node apps/desktop/e2e/viz2-quick.mjs

import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-viz2q-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9358);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
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
const preset = () => page.evaluate(() => window.__sirtoneMilkdrop?.control().preset ?? "");
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("import_paths", {
    paths: [join(root, "fixtures", "generated", "import500", "Air")],
  });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
  await invoke("wave_start", {});
  await invoke("playback_set_repeat", { mode: "all" });
  await sleep(4000);

  // 1. The warp glides.
  const step = await page.evaluate(() => window.__sirtoneWaves?.stats.warpStep ?? -1);
  check(
    "«Волны»: the warp never jumps between frames",
    step >= 0 && step < 0.12,
    `largest step ${step.toFixed(3)}`,
  );

  // 2. MilkDrop: no particles.
  await invoke("settings_patch", {
    patch: { viz: { mode: "milkdrop" }, fx: { mode: "manual", manual: "leaves" } },
  });
  await page.locator('canvas[data-testid="milkdrop"]').waitFor({ timeout: 15000 });
  await sleep(4000);
  check(
    "no live effects over MilkDrop",
    (await page.locator('canvas[data-testid="fx"]').count()) === 0,
  );

  // 3. The lock.
  const before = await preset();
  await page.getByRole("button", { name: "Не менять этот пресет" }).click();
  await sleep(800);
  await invoke("playback_next", {});
  await sleep(2500);
  const afterTrack = await preset();
  await invoke("settings_patch", { patch: { viz: { mode: "waves" } } });
  await sleep(1500);
  await invoke("settings_patch", { patch: { viz: { mode: "milkdrop" } } });
  await page.locator('canvas[data-testid="milkdrop"]').waitFor({ timeout: 15000 });
  await sleep(4000);
  const afterMode = await preset();
  const saved = (await invoke("settings_get", {})).viz.milkdrop_lock;
  check(
    "🔒 holds across a track change and a mode switch, and is saved",
    before && afterTrack === before && afterMode === before && saved === before,
    `${before} → ${afterTrack} → ${afterMode}; saved ${saved}`,
  );
  await page.getByRole("button", { name: "Не менять этот пресет" }).click();
  await sleep(800);

  // 4. «Мои пресеты»: ♥ two, only them.
  await page.getByRole("button", { name: "Режим визуализации" }).click();
  await page.getByRole("menuitem", { name: "Мои пресеты…" }).click();
  const dlg = page.getByRole("dialog");
  await dlg.waitFor();
  const hearts = dlg.getByRole("button", { name: "Нравится этот пресет" });
  const names = await dlg
    .locator("li span")
    .evaluateAll((els) => els.slice(5, 7).map((e) => e.textContent));
  await hearts.nth(5).click();
  await hearts.nth(6).click();
  await dlg.getByRole("switch", { name: "Показывать только мои" }).click();
  await sleep(600);
  await page.keyboard.press("Escape");
  const seen = new Set();
  for (let i = 0; i < 6; i++) {
    await page.getByRole("button", { name: "Следующий пресет" }).click();
    await sleep(700);
    seen.add(await preset());
  }
  check(
    "«Только мои»: only the ♥ presets take turns",
    [...seen].every((n) => names.includes(n)) && seen.size >= 1,
    `mine ${JSON.stringify(names)}, shown ${JSON.stringify([...seen])}`,
  );

  // 5. «Полный экран».
  await page.getByRole("button", { name: "Полный экран", exact: true }).click();
  const full = page.getByTestId("night-vibe");
  await full.waitFor({ timeout: 5000 });
  const title = await full.locator(".t-hero").textContent();
  const hasNext = (await full.getByRole("button", { name: "Вперёд" }).count()) === 1;
  const hasPlay = (await full.getByRole("button", { name: /Пауза|Слушать/ }).count()) === 1;
  await page.keyboard.press("Escape");
  await sleep(800);
  check(
    "«Полный экран»: title, transport; Esc leaves",
    !!title && hasNext && hasPlay && (await page.getByTestId("night-vibe").count()) === 0,
    `title «${title}»`,
  );
} catch (e) {
  check("run", false, String(e).slice(0, 400));
}
app.kill();
process.exit(failed ? 1 : 0);
