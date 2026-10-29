// D-125 «Blend» in the real app: two «House» tracks at 124 BPM in one key (a click on every beat
// over a quiet bass and chord — louder sustained tones blur the beat grid under its 0.8 floor),
// «Как диджей»: the L3 plan hands the bass over at a bar line —
// `blend=true` on the engine's «mix» line — and the handover keeps the device fed (no underrun).
// Isolated instance, volume 0.05. SMOKE_PORT=9351 node apps/desktop/e2e/blend-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-blendq-"));
const libDir = join(dataDir, "Music", "SirTone");
const PORT = Number(process.env.SMOKE_PORT ?? 9351);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const src = join(dataDir, "in", "House");
mkdirSync(src, { recursive: true });
for (const n of [1, 2]) {
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    "aevalsrc='0.6*lt(mod(t,60/124),0.004)+0.03*(sin(2*PI*55*t)+sin(2*PI*110*t)+sin(2*PI*220*t)+sin(2*PI*261.63*t)+sin(2*PI*329.63*t))':s=44100:d=40",
    "-metadata",
    `title=house ${n}`,
    "-metadata",
    "artist=Blend",
    "-metadata",
    "genre=House",
    join(src, `0${n} house ${n}.wav`),
  ]);
}

const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
    SIRTONE_SMOKE_AC: "1",
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
const db = (sql) => {
  const d = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
  try {
    return d.prepare(sql).all();
  } finally {
    d.close();
  }
};
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("playback_set_volume", { volume: 0.05 });
  await invoke("import_paths", { paths: [src] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  let house = [];
  for (let i = 0; i < 240; i++) {
    house = db("SELECT id, title, bpm, key_camelot k FROM track ORDER BY title");
    if (house.length === 2 && house.every((r) => r.bpm && r.k)) break;
    await sleep(1000);
  }
  check(
    "both tracks analysed: tempo and key",
    house.length === 2 && house.every((r) => r.bpm && r.k),
    JSON.stringify(house),
  );
  await invoke("settings_patch", {
    patch: { playback: { automix: { mode: "smart", intensity: "dj" } } },
  });
  await invoke("play_tracks", {
    ids: house.map((h) => h.id),
    start: 0,
    context: "collection",
    playlist: null,
  });
  await sleep(4000); // grids measured, the plan redone
  await invoke("playback_seek", { positionMs: 22000 });
  const u0 = (await invoke("playback_snapshot", {})).underruns;
  await sleep(16000);
  const snap = await invoke("playback_snapshot", {});
  const logDir = join(dataDir, "logs");
  const log = readdirSync(logDir)
    .map((f) => readFileSync(join(logDir, f), "utf8"))
    .join("\n");
  const mix = log.split("\n").filter((l) => /\bmix\b/.test(l) && /blend=/.test(l));
  check(
    "«Blend» (D-125): the bass hands over, B plays, no underrun",
    mix.some((l) => l.includes("blend=true")) &&
      snap.trackId === house[1]?.id &&
      snap.underruns === u0,
    `${mix.at(-1)?.slice(0, 200) ?? "no mix line"}; now ${snap.trackId}, underruns ${u0}→${snap.underruns}`,
  );
  await invoke("playback_toggle", {});
} catch (e) {
  check("run", false, String(e).slice(0, 300));
}
app.kill();
process.exit(failed ? 1 : 0);
