// The iPhone wizard end to end on a real phone (stage 8, M2.1, check П4): an isolated app with
// the dev package (SIRTONE_IOS_APP) and a phone record for an Apple ID whose login is already in
// Windows Credential Manager (`sirtone-cli phone login|adopt`). The wizard must go straight to
// install — no password — and reach «Режим разработчика». Needs the iPhone on the cable.
// SIRTONE_IOS_APP=D:\SirVault\p4-app\src\SirTone.app PHONE_APPLE_ID=<apple id> \
//   SMOKE_PORT=9361 node apps/desktop/e2e/phones-quick.mjs

import { spawn } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = process.env.SMOKE_EXE ?? join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-phonesq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9361);
const appleId = process.env.PHONE_APPLE_ID;
const udid = process.env.PHONE_UDID;
const shot = process.env.SHOT;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
if (!appleId || !udid || !process.env.SIRTONE_IOS_APP) {
  console.log("set SIRTONE_IOS_APP, PHONE_APPLE_ID and PHONE_UDID");
  process.exit(2);
}
writeFileSync(join(dataDir, "phones.json"), JSON.stringify({ [udid]: { appleId } }));
// Past the first-run setup, straight to the app.
writeFileSync(
  join(dataDir, "settings.json"),
  JSON.stringify({ onboarding: { done: true, tips_seen: true } }),
);

const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
    SIRTONE_SMOKE_AC: "1",
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
  },
  stdio: "ignore",
});
let page = null;
for (let i = 0; i < 80 && !page; i++) {
  try {
    const b = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
    page = b.contexts()[0]?.pages()[0] ?? null;
  } catch {
    await sleep(250);
  }
}
try {
  check("app window", page !== null);
  if (page) {
    await page.getByRole("button", { name: "Телефоны", exact: true }).click({ timeout: 20000 });
    const install = page.getByRole("button", { name: "Установить Сиртон" }).first();
    await install.waitFor({ timeout: 20000 });
    check("the hub sees the iPhone", true);
    await install.click();
    await page.getByRole("button", { name: "Дальше" }).click();
    const started = Date.now();
    const bar = page.getByRole("progressbar");
    check("no Apple ID form (stored login)", (await page.getByLabel("Пароль").count()) === 0);
    await bar.waitFor({ timeout: 10000 }).catch(() => {});
    check("progress shown", (await bar.count()) > 0);
    const dev = page.getByText("Включите режим разработчика");
    const err = page.getByRole("alert");
    await Promise.race([dev.waitFor({ timeout: 120000 }), err.waitFor({ timeout: 120000 })]).catch(
      () => {},
    );
    const errText = (await err.count()) ? await err.first().innerText() : "";
    check(
      "installed on the iPhone",
      (await dev.count()) > 0,
      errText || `${((Date.now() - started) / 1000).toFixed(1)} s`,
    );
    if (shot) await page.screenshot({ path: shot });
  }
} catch (e) {
  check("wizard", false, String(e).split("\n")[0]);
} finally {
  app.kill();
}
process.exit(failed ? 1 : 0);
