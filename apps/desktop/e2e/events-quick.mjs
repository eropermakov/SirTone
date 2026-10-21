// Event waves (T5.13–T5.14, AT-13, AT-23, FR-EVT-001…004) in the real app, isolated, with the
// app's clock moved (SIRTONE_SMOKE_SHIFT_MS, honoured only with SIRTONE_DATA_DIR) and the
// «≥ 30 tracks» gate skipped for the tiny library (SIRTONE_SMOKE_EVENTS_ANY):
// 1) five days before Halloween the card comes, once — a restart that day shows none;
// 2) «Включить» starts the event wave, the Halloween theme and fog;
// 3) on 1 November the season's theme is back;
// 4) the first snow of the season (recorded weather, nothing leaves the PC) offers «Первый снег»
//    once — the next snowy day does not.
// SMOKE_PORT=9361 node apps/desktop/e2e/events-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-evq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9361);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};

// Local noon of a date this year → the shift from now.
const shiftTo = (month, day) => {
  const t = new Date();
  const target = new Date(t.getFullYear(), month - 1, day, 12, 0, 0);
  return target.getTime() - t.getTime();
};

// Six quiet tones; one is a direct Halloween match by title.
const src = join(dataDir, "in");
const titles = ["Halloween Night", "Tone 1", "Tone 2", "Tone 3", "Tone 4", "Tone 5"];
titles.forEach((title, i) => {
  const dir = join(src, `Artist ${i}`, "Album");
  mkdirSync(dir, { recursive: true });
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    `sine=frequency=${220 + i * 30}:duration=60`,
    "-af",
    "volume=-30dB",
    "-ac",
    "1",
    "-ar",
    "22050",
    "-c:a",
    "libmp3lame",
    "-b:a",
    "32k",
    "-metadata",
    `title=${title}`,
    "-metadata",
    `artist=Artist ${i}`,
    "-metadata",
    "album=Album",
    join(dir, `t${i}.mp3`),
  ]);
});

// Recorded weather: snow (for the day it is needed), the time set when the app starts.
const net = join(dataDir, "net");
mkdirSync(net);
const snowFixture = (shift) => {
  const now = Math.floor((Date.now() + shift) / 1000);
  writeFileSync(
    join(net, "index.json"),
    JSON.stringify([{ contains: "api.open-meteo.com/v1/forecast", file: "snow.json" }]),
  );
  writeFileSync(
    join(net, "snow.json"),
    JSON.stringify({
      current: {
        time: now,
        weather_code: 73,
        precipitation: 0.4,
        snowfall: 1.4,
        wind_speed_10m: 10,
        wind_direction_10m: 270,
        cloud_cover: 100,
        is_day: 1,
        temperature_2m: -3,
      },
      daily: { sunrise: [now - 15000], sunset: [now + 10000] },
    }),
  );
};

const launch = (shift) =>
  spawn(exe, [], {
    env: {
      ...process.env,
      SIRTONE_DATA_DIR: dataDir,
      SIRTONE_SMOKE_SHIFT_MS: String(shift),
      SIRTONE_SMOKE_EVENTS_ANY: "1",
      SIRTONE_NET_FIXTURES: net,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
    },
    stdio: "ignore",
  });
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
let app;
let page;
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
const start = async (shift) => {
  app = launch(shift);
  page = await connect();
  await page.waitForSelector("footer");
};
const stop = async () => {
  await sleep(2500); // the settings' debounce
  app.kill();
  await sleep(1500);
};
// The events thread looks 8 s after the start, then every minute or on a kick.
const offerWithin = async (ms) => {
  const until = Date.now() + ms;
  for (;;) {
    const o = await invoke("event_offer", {});
    if (o.id || Date.now() > until) return o;
    await sleep(500);
  }
};

try {
  const oct26 = shiftTo(10, 26);
  await start(oct26);
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  const paths = titles.map((_, i) => join(src, `Artist ${i}`));
  await invoke("import_paths", { paths });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 120000 });
  await invoke("event_refresh", {});
  const o1 = await offerWithin(20000);
  check(
    "five days before Halloween the card comes (AT-13)",
    o1.id === "halloween" && o1.daysToPeak === 5,
    JSON.stringify(o1),
  );
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  const card = page.getByTestId("event-card");
  check(
    "the card says «Скоро Хэллоуин» on «Моя волна»",
    (await card.innerText().catch(() => "")).includes("Скоро Хэллоуин"),
  );
  await stop();

  await start(oct26);
  const o2 = await offerWithin(15000);
  check("the same day after a restart — no second card (≤ 1 a day)", o2.id === null);
  // «Включить» on the event the card showed (the tray and the card call the same answer).
  await invoke("event_answer", { id: "halloween", answer: "accept" });
  await sleep(3000);
  const theme = await invoke("theme_state", {});
  const fx = await invoke("fx_sources", {});
  const snap = await invoke("playback_snapshot", {});
  check(
    "«Включить» brings the Halloween theme and fog, the event wave plays",
    theme.source === "event" &&
      theme.id === "halloween" &&
      fx.event === "fog" &&
      snap.waveProfile === "event",
    `${theme.source}/${theme.id} fx=${fx.event} wave=${snap.waveProfile}`,
  );
  await stop();

  await start(shiftTo(11, 1));
  await sleep(10000);
  const t2 = await invoke("theme_state", {});
  const fx2 = await invoke("fx_sources", {});
  check(
    "on 1 November the season is back (AT-13)",
    t2.source === "season" && fx2.event === null,
    `${t2.source}/${t2.id} fx=${fx2.event}`,
  );
  await stop();

  const nov3 = shiftTo(11, 3);
  snowFixture(nov3);
  await start(nov3);
  await invoke("settings_patch", {
    patch: { context: { city: { name: "Москва", lat: 55.75, lon: 37.62 } } },
  });
  const o3 = await offerWithin(30000);
  check(
    "the first snow of the season offers «Первый снег»",
    o3.id === "first_snow",
    JSON.stringify(o3),
  );
  await stop();

  const nov5 = shiftTo(11, 5);
  snowFixture(nov5);
  await start(nov5);
  const o4 = await offerWithin(20000);
  check("the next snowy day — no «Первый снег» again", o4.id !== "first_snow", JSON.stringify(o4));
  await stop();
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
  app?.kill();
}
console.log(failed ? "events-quick: FAILED" : "events-quick: ok");
process.exit(failed ? 1 : 0);
