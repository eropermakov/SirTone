// Sleep timer end to end (T4.21–T4.23, AT-17): an isolated app plays a steady tone on repeat;
// a 2-minute timer with a 1-minute fade and a 10 s final. The loopback level follows the dB line
// (±0.5 dB), a wake in the window brings it back within 1 s, the end pauses (the position
// stays) and the next Play is at full level; the after-timer action runs dry
// (SIRTONE_SMOKE_DRYPOWER: recorded, never done — no test turns the owner's screen off).
// SMOKE_PORT=9355 node apps/desktop/e2e/sleep-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const loopback = join(root, "target", "spikes", "release", "s01-output.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-sleepq-"));
const libDir = join(dataDir, "Music", "SirTone");
const PORT = Number(process.env.SMOKE_PORT ?? 9355);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
if (!existsSync(loopback)) {
  console.log("SKIP  no loopback tool (s01-output.exe)");
  process.exit(0);
}
const src = join(dataDir, "in", "Tone");
mkdirSync(src, { recursive: true });
copyFileSync(join(root, "fixtures", "generated", "signals", "tone-10s.wav"), join(src, "tone.wav"));

const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
    SIRTONE_SMOKE_AC: "1",
    SIRTONE_SMOKE_DRYPOWER: "1",
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
/** Loopback RMS over `secs` seconds. */
const rms = (secs = 1) => {
  const out = spawnSync(loopback, [
    "--capture-pid",
    String(app.pid),
    "--seconds",
    String(secs),
  ]).stdout.toString();
  const m = out.match(/RMS (-?[\d.]+) dBFS/);
  return m ? Number(m[1]) : -999;
};
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("import_paths", { paths: [src] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  const d = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
  const row = d.prepare("SELECT id FROM track LIMIT 1").get();
  d.close();
  await invoke("settings_patch", {
    patch: {
      playback: { normalization: { enabled: false }, automix: { mode: "gapless" } },
      sleep: { fade_min: 1, final_s: 10, warm: false, after: "sleep" },
    },
  });
  await invoke("playback_set_volume", { volume: 0.5 });
  await invoke("playback_set_repeat", { mode: "one" });
  await invoke("play_tracks", { ids: [row.id], start: 0, context: "collection", playlist: null });
  await sleep(1500);
  const ref = rms(2);
  await invoke("sleep_start", { kind: "minutes", value: 2 });
  const t0 = Date.now();
  const at = (s) => sleep(Math.max(0, t0 + s * 1000 - Date.now()));
  // 2 min: fade 50–110 s (0 → −24 dB), final 110–120 s (−24 → −60 dB).
  const line = [];
  for (const s of [40, 65, 80, 95]) {
    await at(s - 0.5);
    const got = rms(1) - ref;
    const want = s < 50 ? 0 : (-24 * (s - 50)) / 60;
    line.push({ s, want: +want.toFixed(2), got: +got.toFixed(2) });
  }
  const status = await invoke("sleep_status", {});
  check(
    "the fade is a straight line in dB (±0.5 dB, AT-17)",
    line.every((p) => Math.abs(p.got - p.want) <= 0.5),
    JSON.stringify(line),
  );
  check("status says fading", status.phase === "fading", JSON.stringify(status));
  // A wake in the window: the volume comes back within 1 s, «+15 мин» on offer.
  await at(100);
  await invoke("sleep_wake", {});
  await sleep(1100);
  const woke = rms(1) - ref;
  const st2 = await invoke("sleep_status", {});
  check(
    "a wake brings the volume back within 1 s",
    Math.abs(woke) <= 0.5 && st2.phase === "woken",
    `${woke.toFixed(2)} dB, ${st2.phase}`,
  );
  // A fresh 1-minute timer to the end (fades over its whole length).
  await invoke("sleep_cancel", {});
  await invoke("sleep_start", { kind: "minutes", value: 1 });
  await sleep(62_000);
  const snap = await invoke("playback_snapshot", {});
  const st3 = await invoke("sleep_status", {});
  check(
    "the end pauses (not stops) and keeps the position",
    snap.state === "paused" && snap.trackId === row.id && st3.phase === "ended",
    `${snap.state} at ${snap.positionMs} ms, ${st3.phase}`,
  );
  const dry = join(dataDir, "sleep-after.txt");
  const action = existsSync(dry) ? readFileSync(dry, "utf8") : "none";
  check(
    "after the timer: «сон» on Modern Standby = screen off (dry run)",
    action === "ScreenOff",
    action,
  );
  await invoke("playback_toggle", {});
  await sleep(1500);
  const again = rms(1) - ref;
  check("the next Play is at full level", Math.abs(again) <= 0.5, `${again.toFixed(2)} dB`);
  await sleep(1500);
  const st4 = await invoke("sleep_status", {});
  check("playing again clears «ended»", st4.phase === "off", st4.phase);
} catch (e) {
  check("run", false, String(e).slice(0, 400));
}
app.kill();
process.exit(failed ? 1 : 0);
