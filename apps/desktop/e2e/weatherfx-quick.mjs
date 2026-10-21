// Weather in the live effects (T5.15, AT-15/16) in the real app; derived from context-quick.
// Context (T5.5, FR-CTX-001/002/004) in the real app, isolated, on a recorded network
// (SIRTONE_NET_FIXTURES — nothing leaves the PC): a city found by name and chosen, the weather
// «снег» with a west wind arrives and is saved; after a restart with the network switched off the
// last weather is still there; without a city there is no weather.
// SMOKE_PORT=9360 node apps/desktop/e2e/context-quick.mjs

import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-wfxq-"));
const net = join(dataDir, "net");
mkdirSync(net);
const now = Math.floor(Date.now() / 1000);
writeFileSync(
  join(net, "index.json"),
  JSON.stringify([
    { contains: "geocoding-api.open-meteo.com/v1/search", file: "geo.json" },
    { contains: "api.open-meteo.com/v1/forecast", file: "snow.json" },
  ]),
);
writeFileSync(
  join(net, "geo.json"),
  JSON.stringify({
    results: [
      { name: "Москва", latitude: 55.75, longitude: 37.62, country: "Россия", admin1: "Москва" },
    ],
  }),
);
writeFileSync(
  join(net, "snow.json"),
  JSON.stringify({
    current: {
      time: now,
      weather_code: 73,
      precipitation: 0.4,
      snowfall: 1.4,
      wind_speed_10m: 18,
      wind_direction_10m: 270,
      cloud_cover: 100,
      is_day: 1,
      temperature_2m: -3.2,
    },
    daily: { sunrise: [now - 20000], sunset: [now + 10000] },
  }),
);
const PORT = Number(process.env.SMOKE_PORT ?? 9362);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const launch = () =>
  spawn(exe, [], {
    env: {
      ...process.env,
      SIRTONE_DATA_DIR: dataDir,
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
const app = launch();
const page = await connect();
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
const fx = () => page.evaluate(() => window.__sirtoneFx?.stats);
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
  await page.locator('canvas[data-testid="fx"]').waitFor({ timeout: 15000 });
  await sleep(1500);
  const season = await fx();
  check(
    "no weather — the season's effect, its own wind (AT-16)",
    season.type !== "snow" && season.wind === null,
    JSON.stringify(season),
  );
  await invoke("settings_patch", {
    patch: {
      context: { city: { name: "Москва", country: null, admin: null, lat: 55.75, lon: 37.62 } },
    },
  });
  for (let i = 0; i < 40 && (await fx()).type !== "snow"; i++) await sleep(500);
  const snow = await fx();
  check(
    "«снег» outside — snow on screen, drifting right with the west wind (AT-15)",
    snow.type === "snow" && snow.wind > 0.3,
    JSON.stringify(snow),
  );
  await invoke("settings_patch", { patch: { context: { city: null } } });
  for (let i = 0; i < 20 && (await fx()).type === "snow"; i++) await sleep(500);
  const back = await fx();
  check(
    "the weather gone — back to the season",
    back.type !== "snow" && back.wind === null,
    JSON.stringify(back),
  );
} catch (e) {
  check("run", false, String(e).slice(0, 400));
}
app.kill();
process.exit(failed ? 1 : 0);
