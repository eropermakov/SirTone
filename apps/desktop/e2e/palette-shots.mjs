// Screenshots of the colour themes in the real app (T7.7, palette 2.0): an isolated instance with
// a small generated library (sine tones, made-up names, generated covers), music very quiet.
// For each theme × colour: «Мой поток» (home), an album, the queue panel with «Сейчас играет»,
// the track menu, settings → «Цвет». Shots: $SHOTS (default target/palette-shots/).
// ACCENTS=sirkov,black,yellow,bordeaux SMOKE_PORT=9381 node apps/desktop/e2e/palette-shots.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = process.env.SMOKE_EXE ?? join(root, "target", "release", "sirtone-app.exe");
const outDir = process.env.SHOTS ?? join(root, "target", "palette-shots");
const ACCENTS = (process.env.ACCENTS ?? "sirkov,black,yellow,bordeaux").split(",");
const THEMES = (process.env.THEMES ?? "dark,light").split(",");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-palette-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9381);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
mkdirSync(outDir, { recursive: true });

// Two albums by made-up artists; covers are generated gradients.
const ALBUMS = [
  { artist: "Северный Свет", album: "Ночная студия", hue: "0x5e0a18", tracks: 5 },
  { artist: "Утро", album: "Бумага", hue: "0x4f9a00", tracks: 3 },
];
for (const a of ALBUMS) {
  const dir = join(dataDir, "in", a.artist, a.album);
  mkdirSync(dir, { recursive: true });
  const cover = join(dir, "cover.jpg");
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    `gradients=s=600x600:c0=${a.hue}:c1=0x110d0c:c2=0xf3ece2:n=3:seed=7`,
    "-frames:v",
    "1",
    cover,
  ]);
  for (let n = 1; n <= a.tracks; n++)
    spawnSync("ffmpeg", [
      "-y",
      "-v",
      "error",
      "-f",
      "lavfi",
      "-i",
      `sine=frequency=${220 + n * 55}:duration=150`,
      "-i",
      cover,
      "-map",
      "0:a",
      "-map",
      "1:v",
      "-af",
      "volume=-40dB",
      "-c:a",
      "libmp3lame",
      "-b:a",
      "32k",
      "-c:v",
      "copy",
      "-id3v2_version",
      "3",
      "-disposition:v",
      "attached_pic",
      "-metadata",
      `title=${["Свет в окне", "Сигарный дым", "Лайм", "Бордо", "Полночь"][n - 1]}`,
      "-metadata",
      `artist=${a.artist}`,
      "-metadata",
      `album=${a.album}`,
      "-metadata",
      `track=${n}`,
      "-metadata",
      "date=2026",
      join(dir, `${n}.mp3`),
    ]);
}

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
const page = browser
  .contexts()[0]
  .pages()
  .find((p) => !p.url().includes("widget"));
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
const settle = () =>
  page.evaluate(() =>
    Promise.race([
      Promise.all(
        document
          .getAnimations()
          .filter((a) => a.effect?.getComputedTiming().iterations !== Number.POSITIVE_INFINITY)
          .map((a) => a.finished.catch(() => undefined)),
      ),
      new Promise((r) => setTimeout(r, 1500)),
    ]),
  );
const nav = (name) => page.getByRole("button", { name, exact: true }).first().click();
const footer = page.locator("footer");
let code = 0;
try {
  console.log("step: onboarding", page.url());
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  console.log("step: import");
  await invoke("import_paths", { paths: ALBUMS.map((a) => join(dataDir, "in", a.artist)) });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 90000 });
  await invoke("playback_set_volume", { volume: 0.05 });
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .getByRole("button", { name: "Закрыть" })
    .click()
    .catch(() => {});
  // Something in the player: the album's second track, paused at 0:42.
  await nav("Альбомы");
  await page.locator("main ul li button").first().click();
  const rows = page.getByRole("list", { name: "Треки" }).locator("button[aria-pressed]");
  await rows.nth(1).dblclick();
  await sleep(1500);
  await invoke("playback_seek", { positionMs: 42000 }).catch(() => {});
  await footer.getByRole("button", { name: "Пауза" }).click();

  for (const theme of THEMES)
    for (const accent of ACCENTS) {
      await invoke("settings_patch", {
        patch: {
          appearance: { theme, accent: { source: "preset", preset: accent, custom: null } },
        },
      });
      await sleep(700);
      const shot = async (name) => {
        console.log("shot", name);
        await settle();
        await sleep(250);
        await page.screenshot({ path: join(outDir, `${theme}-${accent}-${name}.png`) });
      };
      await nav("Мой поток");
      await sleep(800);
      await shot("1-home");
      await nav("Альбомы");
      await page.locator("main ul li button").first().click();
      await rows.first().waitFor({ timeout: 5000 });
      await page.mouse.move(5, 400);
      await shot("2-album");
      await footer.getByRole("button", { name: "Очередь" }).click();
      await page.getByRole("complementary", { name: "Очередь" }).waitFor({ timeout: 3000 });
      await shot("3-now-playing");
      await footer.getByRole("button", { name: "Очередь" }).click();
      await rows.nth(2).click({ button: "right" });
      await page.getByRole("menu").first().waitFor({ timeout: 3000 });
      await shot("4-track-menu");
      await page.keyboard.press("Escape");
      await nav("Настройки");
      await sleep(500);
      const heading = page.getByRole("heading", { name: "Цвет", exact: true }).first();
      if (await heading.count()) await heading.scrollIntoViewIfNeeded();
      else await page.getByText("Цвет", { exact: true }).first().scrollIntoViewIfNeeded();
      await shot("5-settings-color");
      console.log(`ok  ${theme} × ${accent}`);
    }
} catch (e) {
  code = 1;
  console.log("FAIL ", String(e).split("\n")[0]);
  await page.screenshot({ path: join(outDir, "error.png") }).catch(() => {});
}
app.kill();
console.log(`shots: ${outDir}`);
process.exit(code);
