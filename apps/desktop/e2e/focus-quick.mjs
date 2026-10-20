// Focus end to end (T4.24, AT-18 in miniature): an isolated app with 20 quiet tones of
// 40–150 s; a 5/1 focus cycle. The last work track ends −30…+90 s around the boundary (§7.1
// packing, or the crossfade beyond the overrun), the break plays the break wave, then work again;
// today's count +1; normalization is on for the session and off again after «Остановить».
// SMOKE_PORT=9356 node apps/desktop/e2e/focus-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-focusq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9356);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
// 20 tones, 40–150 s, quiet, each its own «artist» so spacing never starves the wave.
const src = join(dataDir, "in");
for (let i = 0; i < 20; i++) {
  const dir = join(src, `Artist ${i}`, "Album");
  mkdirSync(dir, { recursive: true });
  const secs = 40 + ((i * 37) % 111);
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    `sine=frequency=${220 + i * 20}:duration=${secs}`,
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
    `title=Tone ${i}`,
    "-metadata",
    `artist=Artist ${i}`,
    "-metadata",
    "album=Album",
    join(dir, `tone${i}.mp3`),
  ]);
}
const launch = () =>
  spawn(exe, [], {
    env: {
      ...process.env,
      SIRTONE_DATA_DIR: dataDir,
      SIRTONE_SMOKE_AC: "1",
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
  const paths = Array.from({ length: 20 }, (_, i) => join(src, `Artist ${i}`));
  await invoke("import_paths", { paths });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 120000 });
  await invoke("settings_patch", {
    patch: {
      playback: { normalization: { enabled: false }, automix: { mode: "gapless" } },
      focus: { cycles_before_long: 4, overrun_s: 90, vocals: "prefer_none" },
    },
  });
  const t0 = Date.now();
  await invoke("focus_start", { work: 5, brk: 1 });
  await sleep(2500);
  const s0 = await invoke("settings_get", {});
  check(
    "normalization is on for the session (FR-FOCUS-004)",
    s0.playback.normalization.enabled === true,
  );
  // Follow tracks and phases for 7 minutes.
  const spans = [];
  let last = null;
  let boundaryAt = null;
  let breakEndAt = null;
  let today = 0;
  while (Date.now() - t0 < 7 * 60_000) {
    const [snap, st] = await Promise.all([
      invoke("playback_snapshot", {}),
      invoke("focus_status", {}),
    ]);
    const now = (Date.now() - t0) / 1000;
    if (st.phase !== "work" && boundaryAt === null) boundaryAt = now;
    if (boundaryAt !== null && st.phase === "work" && breakEndAt === null) breakEndAt = now;
    today = st.today;
    const key = `${snap.trackId}`;
    if (key !== last?.key) {
      if (last) last.end = now;
      last = { key, title: snap.title, profile: snap.waveProfile, start: now, end: null };
      spans.push(last);
    }
    await sleep(500);
  }
  console.log(
    spans
      .map((s) => `${s.title} [${s.profile}] ${s.start.toFixed(0)}–${s.end?.toFixed(0) ?? "…"} s`)
      .join("\n"),
  );
  const work = spans.filter((s) => s.profile === "focus" && s.start < (boundaryAt ?? 0));
  const lastWork = work.at(-1);
  const off = lastWork?.end != null && boundaryAt != null ? lastWork.end - boundaryAt : null;
  check(
    "the last work track ends −30…+90 s around the boundary (AT-18)",
    off !== null && off >= -30 && off <= 92,
    `boundary ${boundaryAt?.toFixed(0)} s, ends ${off?.toFixed(1)} s off`,
  );
  check(
    "the break plays the break wave",
    spans.some((s) => s.profile === "focus_break"),
    spans.map((s) => s.profile).join(","),
  );
  check(
    "after the break — work again; today +1",
    breakEndAt !== null && today === 1,
    `back at ${breakEndAt?.toFixed(0)} s, today ${today}`,
  );
  await invoke("focus_stop", {});
  await sleep(1500);
  const s1 = await invoke("settings_get", {});
  check(
    "after «Остановить» normalization is off again",
    s1.playback.normalization.enabled === false,
  );
  // The app quits mid-session: the next start gives «off» back (the persisted marker).
  await invoke("focus_start", { work: 25, brk: 5 });
  await sleep(2000);
  const s2 = await invoke("settings_get", {});
  app.kill();
  await sleep(2000);
  app = launch();
  page = await connect();
  await page.waitForSelector("footer");
  await sleep(1500);
  const s3 = await invoke("settings_get", {});
  check(
    "a quit mid-focus does not leave normalization on",
    s2.playback.normalization.enabled === true && s3.playback.normalization.enabled === false,
    `during ${s2.playback.normalization.enabled}, after restart ${s3.playback.normalization.enabled}`,
  );
} catch (e) {
  check("run", false, String(e).slice(0, 400));
}
app.kill();
process.exit(failed ? 1 : 0);
