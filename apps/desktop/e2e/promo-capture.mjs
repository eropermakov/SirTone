// Promo footage of the real app (not a test): an isolated instance with a generated demo library
// (made-up artists, gradient covers, quiet sine chords), recorded scene by scene with the CDP
// screencast at 1600×900. Frames + ffconcat timing per scene land in $PROMO (target/promo/raw);
// scripts/promo-assemble.mjs (or ffmpeg by hand) cuts them into the film.
// SMOKE_PORT=9383 node apps/desktop/e2e/promo-capture.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = process.env.SMOKE_EXE ?? join(root, "target", "release", "sirtone-app.exe");
const outDir = process.env.PROMO ?? join(root, "target", "promo", "raw");
const ONLY = process.env.SCENES?.split(",");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-promo-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9383);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
mkdirSync(outDir, { recursive: true });

// —— Demo library: 8 made-up albums ——
const ALBUMS = [
  ["Северный Свет", "Ночная студия", "0x5e0a18", "0xe39a8f"],
  ["Утро", "Бумага", "0x4f9a00", "0xefe9df"],
  ["Полярный Лис", "Снег на проводах", "0x1d469d", "0x6fa0ff"],
  ["Мята", "Тёплый ламповый", "0x783904", "0xff9a3d"],
  ["Кассета 92", "Плёнка", "0x381b5b", "0xd4a8f0"],
  ["Дом у моря", "Отлив", "0x0d2964", "0x9fd8ff"],
  ["Гранат", "Красное вино", "0x590a0e", "0xff5a5a"],
  ["Лампа", "Янтарь", "0x624802", "0xf5c842"],
];
const TITLES = ["Свет в окне", "Сигарный дым", "Лайм", "Полночь", "Пятый этаж", "Без сети"];
ALBUMS.forEach(([artist, album, c0, c1], ai) => {
  const dir = join(dataDir, "in", artist, album);
  mkdirSync(dir, { recursive: true });
  const cover = join(dir, "cover.jpg");
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    `gradients=s=600x600:c0=${c0}:c1=${c1}:c2=0x110d0c:n=3:seed=${ai + 3}:type=radial`,
    "-frames:v",
    "1",
    cover,
  ]);
  for (let n = 1; n <= 4; n++) {
    const f = 196 + ai * 18 + n * 25;
    spawnSync("ffmpeg", [
      "-y",
      "-v",
      "error",
      "-f",
      "lavfi",
      "-i",
      `aevalsrc=0.2*sin(2*PI*${f}*t)+0.12*sin(2*PI*${f * 1.5}*t)+0.08*sin(2*PI*${f * 1.25}*t):d=200`,
      "-i",
      cover,
      "-map",
      "0:a",
      "-map",
      "1:v",
      "-af",
      "volume=-36dB",
      "-c:a",
      "libmp3lame",
      "-b:a",
      "48k",
      "-c:v",
      "copy",
      "-id3v2_version",
      "3",
      "-disposition:v",
      "attached_pic",
      "-metadata",
      `title=${TITLES[(ai + n) % TITLES.length]}`,
      "-metadata",
      `artist=${artist}`,
      "-metadata",
      `album=${album}`,
      "-metadata",
      `track=${n}`,
      "-metadata",
      "date=2026",
      "-metadata",
      `genre=${["Электроника", "Инди", "Эмбиент", "Джаз"][ai % 4]}`,
      join(dir, `${n}.mp3`),
    ]);
  }
});

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
const cdp = await page.context().newCDPSession(page);
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
const nav = (name) => page.getByRole("button", { name, exact: true }).first().click();
const footer = page.locator("footer");
const W = 1600;
const H = 900;
const viewport = () =>
  cdp.send("Emulation.setDeviceMetricsOverride", {
    width: W,
    height: H,
    deviceScaleFactor: 1.2,
    mobile: false,
  });

/** Records one scene: screencast frames while `act` runs. */
async function scene(name, act) {
  if (ONLY && !ONLY.includes(name)) return;
  const dir = join(outDir, name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const frames = [];
  const onFrame = ({ data, metadata, sessionId }) => {
    frames.push({ data, t: metadata.timestamp });
    cdp.send("Page.screencastFrameAck", { sessionId }).catch(() => {});
  };
  cdp.on("Page.screencastFrame", onFrame);
  await cdp.send("Page.startScreencast", {
    format: "jpeg",
    quality: 92,
    maxWidth: 1920,
    maxHeight: 1080,
  });
  const t0 = Date.now() / 1000;
  await act();
  const t1 = Date.now() / 1000;
  await cdp.send("Page.stopScreencast");
  cdp.off("Page.screencastFrame", onFrame);
  // ffconcat: each frame lasts until the next one; the last until the scene ends.
  let list = "ffconcat version 1.0\n";
  frames.forEach((f, i) => {
    const file = `${String(i).padStart(5, "0")}.jpg`;
    writeFileSync(join(dir, file), Buffer.from(f.data, "base64"));
    const next = frames[i + 1]?.t ?? Math.max(t1, f.t + 0.04);
    list += `file ${file}\nduration ${Math.max(0.001, next - f.t).toFixed(4)}\n`;
  });
  if (frames.length) list += `file ${String(frames.length - 1).padStart(5, "0")}.jpg\n`;
  writeFileSync(join(dir, "list.ffconcat"), list);
  console.log(`scene ${name}: ${frames.length} frames, ${(t1 - t0).toFixed(1)} s`);
}

/** Smooth wheel scroll over the main area. */
async function scroll(dy, steps = 30) {
  await page.mouse.move(W * 0.6, H * 0.55);
  for (let i = 0; i < steps; i++) {
    await page.mouse.wheel(0, dy / steps);
    await sleep(30);
  }
}

const setLook = (theme, preset) =>
  invoke("settings_patch", {
    patch: { appearance: { theme, accent: { source: "preset", preset, custom: null } } },
  });

let code = 0;
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await viewport();
  await setLook("dark", "sirkov");
  await invoke("import_paths", { paths: ALBUMS.map(([a]) => join(dataDir, "in", a)) });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 120000 });
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .getByRole("button", { name: "Закрыть" })
    .click()
    .catch(() => {});
  await invoke("playback_set_volume", { volume: 0.05 });
  // Let the analysis see the tracks (the wave needs them).
  await sleep(20000);
  await viewport();

  // 1. Collection: the album grid, then one album.
  await nav("Альбомы");
  await sleep(1500);
  await scene("collection", async () => {
    await sleep(1200);
    await scroll(260);
    await sleep(700);
    await scroll(-260);
    await sleep(500);
    await page.locator("main ul li button").nth(0).hover();
    await sleep(600);
    await page.locator("main ul li button").nth(0).click();
    await sleep(1600);
    const rows = page.getByRole("list", { name: "Треки" }).locator("button[aria-pressed]");
    await rows.nth(1).hover();
    await sleep(500);
    await rows.nth(1).dblclick();
    await sleep(2200);
  });

  // 2. My wave over the visualiser.
  await invoke("wave_start", { profile: null, query: null }).catch(() => {});
  await nav("Моя волна");
  await sleep(2500);
  await scene("wave", async () => {
    await sleep(6500);
  });

  // 3. «Настроить волну».
  await scene("tune", async () => {
    await sleep(500);
    await page.getByRole("button", { name: "Настроить", exact: true }).first().click();
    await sleep(1800);
    const chips = page.getByRole("dialog").getByRole("button");
    const n = await chips.count();
    for (const i of [2, 5, 9])
      if (i < n) {
        await chips.nth(i).hover();
        await sleep(250);
        await chips
          .nth(i)
          .click()
          .catch(() => {});
        await sleep(700);
      }
    await sleep(800);
    await page.keyboard.press("Escape");
    await sleep(800);
  });

  // 4. Colours: 17 sets and two themes, each change < 100 ms, 400 ms flow.
  await nav("Настройки");
  await sleep(800);
  await page.getByText("Цвет", { exact: true }).first().scrollIntoViewIfNeeded();
  await scroll(-120, 6);
  await sleep(600);
  await scene("colors", async () => {
    await sleep(600);
    for (const name of ["Синий", "Фиолетовый", "Жёлтый", "Оранжевый", "Бордовый"]) {
      const b = page.getByRole("button", { name, exact: true }).first();
      await b.hover();
      await sleep(200);
      await b.click();
      await sleep(750);
    }
    await page.getByRole("button", { name: "Светлая", exact: true }).first().click();
    await sleep(1100);
    for (const name of ["Синий", "Sir.kov"]) {
      const b = page.getByRole("button", { name, exact: true }).first();
      await b.hover();
      await sleep(200);
      await b.click();
      await sleep(900);
    }
    await page.getByRole("button", { name: "Тёмная", exact: true }).first().click();
    await sleep(1300);
  });
  await setLook("dark", "sirkov");

  // 5. Light theme library walk (the morning paper).
  await setLook("light", "sirkov");
  await nav("Исполнители");
  await sleep(1500);
  await scene("light", async () => {
    await sleep(900);
    await nav("Альбомы");
    await sleep(1400);
    await page.locator("main ul li button").nth(2).click();
    await sleep(1500);
    await footer.getByRole("button", { name: "Очередь" }).click();
    await sleep(1800);
    await footer.getByRole("button", { name: "Очередь" }).click();
    await sleep(600);
  });
  await setLook("dark", "sirkov");

  // 6. Full-screen visualiser (the closing shot).
  await nav("Моя волна");
  await sleep(1500);
  await page.getByRole("button", { name: "Полный экран", exact: true }).first().click();
  await sleep(1500);
  await scene("viz", async () => {
    await sleep(7000);
  });
} catch (e) {
  code = 1;
  console.log("FAIL ", String(e).split("\n")[0]);
  await page.screenshot({ path: join(outDir, "error.png") }).catch(() => {});
}
await invoke("playback_pause", {}).catch(() => {});
app.kill();
console.log(`raw: ${outDir}`);
process.exit(code);
