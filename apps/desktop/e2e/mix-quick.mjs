// Automix / crossfade end to end (T4.9, FR-PLAY-010): an isolated app, two analysed tracks,
// «Кроссфейд 4 с»; through the handover the device never goes silent (loopback), the player
// moves to B, and the journal ends A as «mixed out» (a complete listen, not a skip).
// SMOKE_PORT=9350 node apps/desktop/e2e/mix-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const loopback = join(root, "target", "spikes", "release", "s01-output.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-mixq-"));
const libDir = join(dataDir, "Music", "SirTone");
const PORT = Number(process.env.SMOKE_PORT ?? 9350);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const src = join(dataDir, "in", "Mix");
mkdirSync(src, { recursive: true });
const sig = join(root, "fixtures", "generated", "signals");
copyFileSync(join(sig, "level-14.wav"), join(src, "01 first.wav"));
copyFileSync(join(sig, "level-20.wav"), join(src, "02 second.wav"));

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
  await invoke("import_paths", { paths: [src] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  let rows = [];
  for (let i = 0; i < 240; i++) {
    rows = db("SELECT id, title, env_tail IS NOT NULL AS env FROM track ORDER BY title");
    if (rows.length === 2 && rows.every((r) => r.env)) break;
    await sleep(1000);
  }
  check(
    "both tracks analysed",
    rows.length === 2 && rows.every((r) => r.env),
    JSON.stringify(rows),
  );
  await invoke("settings_patch", {
    patch: { playback: { automix: { mode: "crossfade", crossfade_s: 4 } } },
  });
  await invoke("playback_set_volume", { volume: 0.5 });
  const [a, b] = rows;
  await invoke("play_tracks", {
    ids: [a.id, b.id],
    start: 0,
    context: "collection",
    playlist: null,
  });
  await sleep(800);
  // 5 s before A's end: the crossfade starts at 6 s, A ends at 10 s.
  await invoke("playback_seek", { positionMs: 5000 });
  const cap = existsSync(loopback)
    ? spawnSync(loopback, ["--capture-pid", String(app.pid), "--seconds", "7"]).stdout.toString()
    : "";
  const m = cap.match(/10 ms silent blocks inside signal: (\d+)/);
  check(
    "no silence through the crossfade (loopback)",
    !existsSync(loopback) || (m && m[1] === "0"),
    m ? m[0] : cap.trim().slice(0, 200),
  );
  const snap = await invoke("playback_snapshot", {});
  check("the player moved to B", snap.trackId === b.id, `track ${snap.trackId}`);
  await invoke("playback_toggle", {});
  await sleep(1500);
  const ev = db(`SELECT track_id, end_reason, played_ms FROM play_event ORDER BY id`);
  const first = ev.find((e) => e.track_id === a.id);
  check("the journal ends A as «mixed out»", first?.end_reason === "mixed_out", JSON.stringify(ev));
  // T4.9b: the manual «Следующий» — B is current at once, the cut is a short fade (no silence),
  // and A is a skip.
  await invoke("play_tracks", {
    ids: [a.id, b.id],
    start: 0,
    context: "collection",
    playlist: null,
  });
  await sleep(1500);
  const capNext = existsSync(loopback)
    ? spawn(loopback, ["--capture-pid", String(app.pid), "--seconds", "3"])
    : null;
  let capOut = "";
  capNext?.stdout.on("data", (d) => {
    capOut += d;
  });
  await sleep(700);
  const t0 = Date.now();
  await invoke("playback_next", {});
  let switched = false;
  while (!switched && Date.now() - t0 < 300) {
    switched = (await invoke("playback_snapshot", {})).trackId === b.id;
  }
  const dt = Date.now() - t0;
  await new Promise((r) => (capNext ? capNext.on("exit", r) : r()));
  const mn = capOut.match(/10 ms silent blocks inside signal: (\d+)/);
  check(
    "manual next: B at once, a short fade without silence",
    switched && dt < 150 && (!capNext || (mn && mn[1] === "0")),
    `${dt} ms, ${mn ? mn[0] : capOut.trim().slice(0, 120)}`,
  );
  // «Смешать сейчас» in automix mode: B comes in ~1.5 s later and A is mixed out.
  await invoke("settings_patch", { patch: { playback: { automix: { mode: "smart" } } } });
  await invoke("play_tracks", {
    ids: [a.id, b.id],
    start: 0,
    context: "collection",
    playlist: null,
  });
  await sleep(1200);
  const mixed = await invoke("playback_mix_now", {});
  await sleep(2500);
  const afterMix = (await invoke("playback_snapshot", {})).trackId;
  await invoke("playback_toggle", {});
  await sleep(1500);
  const ev2 = db("SELECT track_id, end_reason FROM play_event ORDER BY id");
  check(
    "«Смешать сейчас»: B comes in, A is mixed out",
    mixed === true &&
      afterMix === b.id &&
      ev2.at(-1)?.track_id === a.id &&
      ev2.at(-1)?.end_reason === "mixed_out",
    `mixed ${mixed}, now ${afterMix}, ${JSON.stringify(ev2.slice(-3))}`,
  );
} catch (e) {
  check("run", false, String(e).slice(0, 300));
}
app.kill();
process.exit(failed ? 1 : 0);
