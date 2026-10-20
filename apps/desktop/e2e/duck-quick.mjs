// Ducking scenarios (T4.14, spike S-11, FR-PLAY-011): an isolated app plays a steady tone; other
// "applications" (PowerShell SoundPlayer processes) make sound or not; the loopback level of
// Sirtone's own process shows whether it ducked. False ducks are the failure to avoid.
// SMOKE_PORT=9351 node apps/desktop/e2e/duck-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const loopback = join(root, "target", "spikes", "release", "s01-output.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-duckq-"));
const libDir = join(dataDir, "Music", "SirTone");
const PORT = Number(process.env.SMOKE_PORT ?? 9351);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = 0;
const results = [];
const check = (name, ok, detail = "") => {
  if (!ok) failed++;
  results.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
if (!existsSync(loopback)) {
  console.log("SKIP  no loopback tool");
  process.exit(0);
}

// 16-bit mono 48 kHz WAVs for the other "applications".
const wav = (path, secs, amp, freq = 1000) => {
  const n = Math.round(48000 * secs);
  const b = Buffer.alloc(44 + n * 2);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(1, 22);
  b.writeUInt32LE(48000, 24);
  b.writeUInt32LE(96000, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) {
    b.writeInt16LE(
      Math.round(amp * 32767 * Math.sin((2 * Math.PI * freq * i) / 48000)),
      44 + i * 2,
    );
  }
  writeFileSync(path, b);
  return path;
};
const work = join(dataDir, "work");
mkdirSync(work, { recursive: true });
const loud = wav(join(work, "loud.wav"), 3, 0.08);
const ding = wav(join(work, "ding.wav"), 0.3, 0.08, 1500);
const silent = wav(join(work, "silent.wav"), 3, 0);
// Another application: PowerShell playing a file (looping or a number of single plays).
const other = (file, mode) =>
  spawn(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      mode === "loop"
        ? `$p = New-Object Media.SoundPlayer '${file}'; $p.PlayLooping(); Start-Sleep 60`
        : `$p = New-Object Media.SoundPlayer '${file}'; 1..6 | % { $p.PlaySync(); Start-Sleep -Milliseconds 700 }`,
    ],
    { stdio: "ignore" },
  );

const src = join(dataDir, "in", "Tone");
mkdirSync(src, { recursive: true });
copyFileSync(join(root, "fixtures", "generated", "signals", "tone-10s.wav"), join(src, "tone.wav"));
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
const rms = (secs = 1) => {
  const out = spawnSync(loopback, [
    "--capture-pid",
    String(app.pid),
    "--seconds",
    String(secs),
  ]).stdout.toString();
  const m = out.match(/RMS (-?[\d.]+) dBFS/);
  return m ? Number(m[1]) : null;
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
  const id = d.prepare("SELECT id FROM track LIMIT 1").get().id;
  d.close();
  await invoke("settings_patch", {
    patch: {
      playback: {
        normalization: { enabled: false },
        automix: { mode: "gapless" },
        duck: { enabled: true, amount_db: -12, sensitivity: "medium", ignore: [] },
      },
    },
  });
  await invoke("playback_set_volume", { volume: 0.5 });
  await invoke("playback_set_repeat", { mode: "one" });
  await invoke("play_tracks", { ids: [id], start: 0, context: "collection", playlist: null });
  await sleep(1500);
  const base = rms();
  check("baseline level", base !== null && base > -60, `${base} dBFS`);

  // 1. Another app plays continuously → ducked (≈ −12 dB) …
  const p1 = other(loud, "loop");
  await sleep(2500);
  const ducked = rms();
  check(
    "1. another app plays → Sirtone ducks ≈ 12 dB",
    base - ducked >= 9,
    `${ducked} dBFS (Δ ${(base - ducked).toFixed(1)})`,
  );
  // 2. … and comes back after it stops.
  p1.kill();
  spawnSync("taskkill", ["/PID", String(p1.pid), "/T", "/F"]);
  await sleep(3500);
  const back = rms();
  check("2. it stops → back within 3.5 s", Math.abs(back - base) <= 1.5, `${back} dBFS`);

  // 3. Short notifications (0.3 s every second) → never.
  const p3 = other(ding, "dings");
  const notif = [];
  for (let i = 0; i < 4; i++) notif.push(rms(1));
  p3.kill();
  check(
    "3. short notifications never duck",
    notif.every((v) => base - v < 3),
    notif.join(" / "),
  );
  await sleep(2000);

  // 4. An open but silent session (a player on silence) → never.
  const p4 = other(silent, "loop");
  await sleep(2500);
  const quiet = rms();
  spawnSync("taskkill", ["/PID", String(p4.pid), "/T", "/F"]);
  check("4. a silent foreign session never ducks", base - quiet < 3, `${quiet} dBFS`);
  await sleep(1500);

  // 5. Sirtone's own WebView playing sound → never (own process tree).
  await page.evaluate(() => {
    const ctx = new AudioContext();
    const o = ctx.createOscillator();
    const g = ctx.createGain();
    g.gain.value = 0.2;
    o.connect(g).connect(ctx.destination);
    o.start();
    window.__ownTone = ctx;
  });
  await sleep(2500);
  const own = rms();
  await page.evaluate(() => window.__ownTone?.close());
  // Own sound adds to our own loopback; a duck would pull the tone 12 dB down.
  check("5. Sirtone's own WebView sound never ducks it", own - base > -3, `${own} dBFS`);
  await sleep(1500);

  // 6. An excepted app (powershell.exe in the list) → never.
  await invoke("settings_patch", { patch: { playback: { duck: { ignore: ["powershell.exe"] } } } });
  const p6 = other(loud, "loop");
  await sleep(2500);
  const excepted = rms();
  spawnSync("taskkill", ["/PID", String(p6.pid), "/T", "/F"]);
  check("6. an excepted app never ducks", base - excepted < 3, `${excepted} dBFS`);

  // 7. Ducking off → never.
  await invoke("settings_patch", {
    patch: { playback: { duck: { enabled: false, ignore: [] } } },
  });
  const p7 = other(loud, "loop");
  await sleep(2500);
  const off = rms();
  spawnSync("taskkill", ["/PID", String(p7.pid), "/T", "/F"]);
  check("7. switched off → never", base - off < 3, `${off} dBFS`);
} catch (e) {
  check("run", false, String(e).slice(0, 300));
}
app.kill();
console.log(`${results.filter(Boolean).length}/${results.length}`);
process.exit(failed ? 1 : 0);
