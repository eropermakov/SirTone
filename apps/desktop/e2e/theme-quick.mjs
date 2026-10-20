// Auto-degrade (T4.16) and seasonal themes (T4.18) in the real app, isolated data dir.
// 1) Slow frames (a 30 ms busy wait in every rAF) → the level drops by one after ≈ 5 s, sticks
//    in the settings, the toast offers «Вернуть», which restores it.
// 2) «Тема по сезону» on → the accent comes from today's seed (`theme_state`), no long task over
//    50 ms during the switch (P-19); a pick is this theme's own accent (Q-15); the season swatch
//    returns to the theme's colour; off → the user's preset again.
// SMOKE_PORT=9353 node apps/desktop/e2e/theme-quick.mjs

import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-themeq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9353);
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
const settings = () => invoke("settings_get", {});
const accentFill = () =>
  page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--accent-fill").trim(),
  );
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  await sleep(1500);

  // —— T4.16 ——
  const before = (await settings()).performance.level;
  // Count settings changes over 6 s of nothing: a background writer would keep resetting the
  // grace period and auto-degrade would never fire.
  await page.evaluate(() => {
    window.__changes = 0;
    window.__TAURI_INTERNALS__.invoke("plugin:event|listen", {
      event: "settings://changed",
      target: { kind: "Any" },
      handler: window.__TAURI_INTERNALS__.transformCallback(() => {
        window.__changes++;
      }),
    });
  });
  await page.evaluate(() => {
    window.__slow = true;
    const burn = () => {
      if (!window.__slow) return;
      const t = performance.now();
      while (performance.now() - t < 30) {}
      requestAnimationFrame(burn);
    };
    requestAnimationFrame(burn);
  });
  let after = before;
  const t0 = Date.now();
  while (Date.now() - t0 < 12000 && after === before) {
    await sleep(500);
    after = (await settings()).performance.level;
  }
  const took = Date.now() - t0;
  await page.evaluate(() => {
    window.__slow = false;
  });
  const s1 = await settings();
  check(
    "slow frames lower the level by one after ≈ 5 s",
    after === before - 1 && took >= 4000 && took <= 9000,
    `level ${before} → ${after} in ${took} ms, degraded_from ${s1.performance.degraded_from}, settings events ${await page.evaluate(() => window.__changes)}`,
  );
  const restore = page.getByRole("button", { name: "Вернуть", exact: true });
  check(
    "the toast offers «Вернуть»",
    (await restore.count()) > 0,
    `${await restore.count()} button(s)`,
  );
  if ((await restore.count()) > 0) {
    await restore.first().click();
    for (let i = 0; i < 10 && (await settings()).performance.level !== before; i++)
      await sleep(300);
    const s2 = await settings();
    check(
      "«Вернуть» restores the chosen level",
      s2.performance.level === before && s2.performance.degraded_from === null,
      JSON.stringify(s2.performance),
    );
  }
  await sleep(3000);
  const level = (await settings()).performance.level;
  check("smooth again — no further drop", level === before, `level ${level}`);

  // —— T4.18 ——
  const plain = await accentFill();
  const state = await invoke("theme_state", {});
  await page.evaluate(() => {
    window.__long = [];
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) window.__long.push(e.duration);
    }).observe({ type: "longtask" });
  });
  await page.getByRole("switch", { name: "Тема по сезону" }).first().click();
  await sleep(1500);
  const seasonal = await accentFill();
  const long = await page.evaluate(() => window.__long);
  check(
    "«Тема по сезону» paints the season's colour",
    seasonal !== plain && (await page.getByRole("button", { name: "Цвет сезона" }).count()) > 0,
    `${state.id}/${state.card} seed ${state.seed}: ${plain} → ${seasonal}`,
  );
  check(
    "P-19: no long task > 50 ms on the theme change",
    long.every((d) => d <= 50),
    `long tasks ${JSON.stringify(long.map((d) => Math.round(d)))}`,
  );
  await page.getByRole("button", { name: "Синий", exact: true }).first().click();
  await sleep(1200);
  const s3 = await settings();
  check(
    "a pick is this theme's own accent (Q-15), the base preset stays",
    s3.appearance.theme_accents?.[state.id]?.preset === "blue" &&
      s3.appearance.accent.preset === "sirkov",
    JSON.stringify(s3.appearance),
  );
  const blue = await accentFill();
  await page.getByRole("button", { name: "Цвет сезона" }).first().click();
  await sleep(1200);
  check("the season swatch returns to the theme's colour", (await accentFill()) === seasonal, blue);
  await page.getByRole("switch", { name: "Тема по сезону" }).first().click();
  await sleep(1200);
  check("off — the user's preset again", (await accentFill()) === plain);
} catch (e) {
  check("run", false, String(e).slice(0, 300));
}
app.kill();
process.exit(failed ? 1 : 0);
