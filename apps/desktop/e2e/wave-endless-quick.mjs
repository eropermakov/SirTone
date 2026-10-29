// «Моя волна» never ends (T7.2 soak finding, D-119) in the real app, isolated: a library shorter
// than the 24 h repeat window — 5 tracks of 8 s — must keep playing after all of them were heard
// (the least recently played come back first). Then the soak's hourly chores one by one with the
// music playing: closed to the tray until the window is destroyed (60 s), built again, the output
// reopened on the default device by id and back — playback must stay «playing» after each.
// SMOKE_PORT=9377 node apps/desktop/e2e/wave-endless-quick.mjs

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { closeMainScript } from "./close-main.mjs";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-endlessq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9377);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const dir = join(dataDir, "in", "Short");
mkdirSync(dir, { recursive: true });
for (let n = 0; n < 5; n++)
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    `sine=frequency=${220 + n * 70}:duration=8`,
    "-af",
    "volume=-40dB",
    "-c:a",
    "libmp3lame",
    "-b:a",
    "64k",
    "-metadata",
    `title=Short ${n}`,
    "-metadata",
    `artist=Short Artist ${n}`,
    "-metadata",
    "album=Short Album",
    join(dir, `short-${n}.mp3`),
  ]);
const env = {
  ...process.env,
  SIRTONE_DATA_DIR: dataDir,
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
};
const app = spawn(exe, [], { env, stdio: "ignore" });
const ps = (script) =>
  execFileSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).trim();
// WM_CLOSE to this test's own main window (D-124: not Process.CloseMainWindow).
const closeMain = () => {
  if (ps(closeMainScript(app.pid)) !== "closed") throw new Error("no main window to close");
};
let browser = null;
const connect = async () => {
  await browser?.close().catch(() => {});
  for (let i = 0; i < 120; i++) {
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      const p = browser
        .contexts()
        .flatMap((c) => c.pages())
        .find((x) => !x.url().includes("widget"));
      if (p) return p;
    } catch {
      await sleep(250);
    }
  }
  throw new Error("no page");
};
let page = await connect();
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
const snap = () => invoke("playback_snapshot", {});
try {
  await invoke("settings_patch", {
    patch: {
      onboarding: { done: true, tips_seen: true },
      playback: { automix: { mode: "gapless" } },
    },
  });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("import_paths", { paths: [dir] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  await invoke("playback_set_volume", { volume: 0.05 });
  await invoke("wave_start", { profile: null, query: null });

  // —— 5 × 8 s = 40 s of music; 75 s later the wave must still play ——
  const heard = [];
  let stopped = null;
  const t0 = Date.now();
  while (Date.now() - t0 < 75000) {
    await sleep(1000);
    const s = await snap();
    if (s.title && heard.at(-1) !== s.title) heard.push(s.title);
    if (s.state !== "playing") {
      stopped = `${s.state} after ${((Date.now() - t0) / 1000).toFixed(0)} s`;
      break;
    }
  }
  check(
    "a library shorter than the repeat window: the wave goes on after every track was heard",
    stopped === null && heard.length >= 7,
    `${stopped ?? "playing"}; heard ${heard.length}: ${heard.join(", ")}`,
  );
  const once = heard.slice(0, 5);
  check("all five once before any comes back", new Set(once).size === 5, once.join(", "));
  if (stopped) await invoke("wave_start", { profile: null, query: null });

  // —— the soak's hourly chores, one at a time ——
  const state = async (step) => {
    await sleep(1500);
    const s = await snap();
    check(`after ${step}: still playing`, s.state === "playing", `${s.state} «${s.title}»`);
  };
  closeMain();
  await sleep(70000); // the window is destroyed after 60 s in the tray
  spawn(exe, [], { env, stdio: "ignore" });
  page = await connect();
  await page.waitForSelector("footer", { timeout: 20000 });
  await state("70 s in the tray and the window built again");
  const def = (await invoke("audio_devices", {})).find((d) => d.isDefault);
  await invoke("settings_patch", { patch: { playback: { output: { device: def.id } } } });
  await state("the output reopened on the default device by id");
  await invoke("settings_patch", { patch: { playback: { output: { device: "default" } } } });
  await state("back to «Как в Windows»");
  // The soak's underruns came ≈ 22 s after a track started behind two reopens: listen past that.
  const u0 = (await snap()).underruns;
  await invoke("playback_next", {});
  await sleep(35000);
  const s1 = await snap();
  check(
    "no underrun in the 35 s after the reopens and a new track (P-13)",
    s1.underruns === u0 && s1.state === "playing",
    `underruns ${u0} → ${s1.underruns}, ${s1.state}`,
  );
  // A reopen is a planned gap (D-121): the whole run, tray and reopens included, is clean.
  check("no underrun over the whole run (P-13)", s1.underruns === 0, `${s1.underruns}`);
  await invoke("playback_toggle", {});
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await sleep(1500);
app.kill();
console.log(failed ? "wave-endless-quick: FAILED" : "wave-endless-quick: ok");
process.exit(failed ? 1 : 0);
