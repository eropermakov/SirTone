// Context (T5.5, FR-CTX-001/002/004) in the real app, isolated, on a recorded network
// (SIRTONE_NET_FIXTURES — nothing leaves the PC): a city found by name and chosen, the weather
// «снег» with a west wind arrives and is saved; after a restart with the network switched off the
// last weather is still there; without a city there is no weather.
// SMOKE_PORT=9360 node apps/desktop/e2e/context-quick.mjs

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-ctxq-"));
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
const PORT = Number(process.env.SMOKE_PORT ?? 9360);
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
let app = launch();
let page = await connect();
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  const none = await invoke("weather_get", {});
  check("no city — no weather", none.class === null, JSON.stringify(none));
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  await page.getByRole("searchbox", { name: "Найти город" }).fill("Моск");
  await page
    .getByRole("button", { name: /Москва/ })
    .first()
    .click({ timeout: 10000 });
  const line = page.getByTestId("place-weather");
  await line.filter({ hasText: "снег" }).waitFor({ timeout: 30000 });
  const w = await invoke("weather_get", {});
  check(
    "the city is chosen, the weather «снег» arrives with the wind",
    w.class === "snow" && w.fresh && w.drift > 0.3 && Math.round(w.tempC) === -3,
    `${await line.textContent()} · ${JSON.stringify(w)}`,
  );
  check("the weather is saved", existsSync(join(dataDir, "weather.json")));
  // Offline after a restart: the last weather stays (FR-CTX-002 «без сети — последнее известное»).
  await invoke("settings_patch", { patch: { network: { offline_mode: true } } });
  // Settings reach the disk after a short debounce; a hard kill must not come first.
  await sleep(2500);
  app.kill();
  await sleep(2000);
  app = launch();
  page = await connect();
  await page.waitForSelector("footer");
  await sleep(1500);
  const again = await invoke("weather_get", {});
  check(
    "offline after a restart — the last weather",
    again.class === "snow",
    JSON.stringify(again),
  );
  // No city — no weather again.
  await invoke("settings_patch", { patch: { context: { city: null } } });
  await sleep(800);
  const cleared = await invoke("weather_get", {});
  check("the city removed — no weather", cleared.class === null, JSON.stringify(cleared));
} catch (e) {
  check("run", false, String(e).slice(0, 400));
}
app.kill();
process.exit(failed ? 1 : 0);
