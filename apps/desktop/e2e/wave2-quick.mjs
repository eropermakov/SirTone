// «Настроить волну» 2.0 and friends in the real app, isolated (T5.7, T5.11, T5.16):
// 1) the live counter «Подходит N треков» counts the library and narrows with a filter;
// 2) «Волна по треку» plays the track, then keeps going past where a playlist would stop;
// 3) «Состав волны»: the chips name the context, a pressed chip stays off for the session and a
//    new wave starts with all on; 4) the contextual name says when it changes next.
// SMOKE_PORT=9362 node apps/desktop/e2e/wave2-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-wave2q-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9362);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const N = 12;
const src = join(dataDir, "in");
for (let i = 0; i < N; i++) {
  const dir = join(src, `Artist ${i}`, "Album");
  mkdirSync(dir, { recursive: true });
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    `sine=frequency=${200 + i * 40}:duration=40`,
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
    join(dir, `t${i}.mp3`),
  ]);
}
const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
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
await page.waitForSelector("footer");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("import_paths", {
    paths: Array.from({ length: N }, (_, i) => join(src, `Artist ${i}`)),
  });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 120000 });

  const all = await invoke("wave_query_count", { query: {} });
  const loved = await invoke("wave_query_count", { query: { frequency: ["loved"] } });
  check(
    "«Подходит N треков»: the whole library, and a filter narrows or widens honestly",
    all?.tracks === N && !all.relaxed && (loved.tracks < N || loved.relaxed),
    `${JSON.stringify(all)} ${JSON.stringify(loved)}`,
  );

  await invoke("wave_start", { profile: null, query: null });
  await sleep(1500);
  const seed = (await invoke("playback_snapshot", {})).trackId;
  await invoke("wave_by", { kind: "track", id: seed });
  await sleep(2500);
  const s0 = await invoke("playback_snapshot", {});
  let q = await invoke("queue_get", {});
  check(
    "«Волна по треку» plays the track first",
    s0.trackId === seed && q.items.length >= 2,
    `${s0.trackId}/${seed} queue ${q.items.length}`,
  );
  const heard = new Set([seed]);
  for (let k = 0; k < 8; k++) {
    await invoke("playback_next", {});
    await sleep(1200);
    heard.add((await invoke("playback_snapshot", {})).trackId);
  }
  q = await invoke("queue_get", {});
  check(
    "…and keeps going — never stops like a playlist continuation",
    heard.size >= 8 && q.items.length > q.position + 1,
    `heard ${heard.size}, queue ${q.items.length} at ${q.position}`,
  );

  await invoke("wave_start", { profile: null, query: null });
  await sleep(1500);
  const c0 = await invoke("wave_composition", {});
  await invoke("wave_set_off", { off: { ...c0.off, taste: true } });
  await sleep(1500);
  const c1 = await invoke("wave_composition", {});
  await invoke("wave_start", { profile: null, query: null });
  await sleep(1500);
  const c2 = await invoke("wave_composition", {});
  check(
    "«Состав волны»: season and time named; «Твой вкус» off for the session, on again in a new wave",
    !!c0.season && !!c0.daypart && c1.off.taste === true && c2.off.taste === false,
    `${c0.season}/${c0.daypart} ${c1.off.taste} ${c2.off.taste}`,
  );

  const name = await invoke("wave_context_name", {});
  check(
    "the contextual name: part of the day and when it changes next",
    !!name.daypart && name.nextAt > Date.now() && name.nextAt < Date.now() + 26 * 3_600_000,
    JSON.stringify(name),
  );
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  const shown = await page
    .getByTestId("wave-context")
    .innerText()
    .catch(() => "");
  check("the name is on «Моя волна»", /Обновится в/.test(shown), shown.replace(/\n/g, " · "));
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await sleep(2500);
app.kill();
console.log(failed ? "wave2-quick: FAILED" : "wave2-quick: ok");
process.exit(failed ? 1 : 0);
