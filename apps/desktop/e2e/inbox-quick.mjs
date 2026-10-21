// «Папка Inbox» (T6.8, FR-IMP-010) in the real app, isolated: switched on — `Inbox` appears in the
// library folder; a song copied there (as Explorer would) is taken in and laid out under
// `Артист/Альбом` by itself, nothing stays in Inbox. Only the test's temp folders are touched.
// SMOKE_PORT=9368 node apps/desktop/e2e/inbox-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-inboxq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9368);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const src = join(dataDir, "in");
mkdirSync(join(src, "First", "Album"), { recursive: true });
const tone = (dir, title, artist, album) =>
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    "sine=frequency=300:duration=10",
    "-c:a",
    "libmp3lame",
    "-b:a",
    "32k",
    "-metadata",
    `title=${title}`,
    "-metadata",
    `artist=${artist}`,
    "-metadata",
    `album=${album}`,
    "-metadata",
    "track=1",
    join(dir, `${title}.mp3`),
  ]);
tone(join(src, "First", "Album"), "Seed", "First", "Album");
const dropped = join(dataDir, "dropped");
mkdirSync(dropped);
tone(dropped, "Dropped Song", "Postman", "Letters");
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
  await invoke("import_paths", { paths: [join(src, "First")] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  const libRoot = (await invoke("library_status", {})).root;
  await invoke("settings_patch", { patch: { library: { inbox: true } } });
  await sleep(1000);
  const inbox = join(libRoot, "Inbox");
  check("«Папка Inbox» on — the folder appears", existsSync(inbox), inbox);
  copyFileSync(join(dropped, "Dropped Song.mp3"), join(inbox, "Dropped Song.mp3"));
  let placed = false;
  for (let i = 0; i < 40 && !placed; i++) {
    await sleep(500);
    placed = existsSync(join(libRoot, "Postman", "Letters", "01 - Dropped Song.mp3"));
  }
  await sleep(500);
  const left = existsSync(inbox) ? readdirSync(inbox) : ["(Inbox gone)"];
  const status = await invoke("library_status", {});
  check(
    "a song put into Inbox is taken in and laid out under Артист/Альбом by itself; Inbox stays, empty",
    placed && left.length === 0 && status.tracks === 2,
    `placed ${placed}, left in Inbox: ${left.join(", ") || "nothing"}, tracks ${status.tracks}`,
  );
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await sleep(2500);
app.kill();
console.log(failed ? "inbox-quick: FAILED" : "inbox-quick: ok");
process.exit(failed ? 1 : 0);
