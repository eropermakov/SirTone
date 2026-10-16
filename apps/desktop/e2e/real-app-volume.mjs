// Volume survives a restart (D-64), on the built exe over WebView2 DevTools (port 9333).
//   node real-app-volume.mjs set <0..1>   — sets the volume through the app
//   node real-app-volume.mjs check <0..1> — after a restart: the slider shows that value
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const [mode, raw] = process.argv.slice(2);
const value = Number(raw);
const browser = await chromium.connectOverCDP("http://127.0.0.1:9333");
const page = browser.contexts()[0].pages()[0];
let ok = true;
if (mode === "set") {
  await page.evaluate(
    (v) => window.__TAURI_INTERNALS__.invoke("playback_set_volume", { volume: v }),
    value,
  );
  await page.waitForTimeout(1600); // settings are written at most once per second
  console.log(`set volume ${value}`);
} else {
  const slider = page.getByRole("slider", { name: "Громкость" });
  await slider.waitFor();
  await page.waitForTimeout(500);
  const now = Number(await slider.getAttribute("aria-valuenow"));
  ok = now === Math.round(value * 100);
  console.log(
    `${ok ? "PASS" : "FAIL"}  volume after restart — slider ${now}, expected ${Math.round(value * 100)}`,
  );
}
await browser.close();
process.exit(ok ? 0 : 1);
