// Portable Сиртон (T6.4, FR-PORT-001/002) in the real app: scripts/portable.ps1 builds the folder
// into a temp dir; started from there (no SIRTONE_DATA_DIR) it keeps settings and logs in .\Data
// and the music in .\Music, stores the library path relative to the folder; after the folder is
// moved (another drive letter on another PC) the library is still found. Nothing is written to
// the host: no Run entry for it, no updater. The owner's installed Сиртон is never touched (its
// own single-instance identity).
// SMOKE_PORT=9367 node apps/desktop/e2e/portable-quick.mjs

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const work = mkdtempSync(join(tmpdir(), "sirtone-portq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9367);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const out = execFileSync(
  "powershell",
  [
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    join(root, "scripts", "portable.ps1"),
    "-Out",
    work,
  ],
  { encoding: "utf8" },
);
const folder = out.match(/portable -> (.+)/)?.[1]?.trim();
const src = join(work, "in", "Artist", "Album");
mkdirSync(src, { recursive: true });
for (const n of [1, 2])
  spawnSync("ffmpeg", [
    "-y",
    "-v",
    "error",
    "-f",
    "lavfi",
    "-i",
    `sine=frequency=${300 + n * 50}:duration=20`,
    "-af",
    "volume=-40dB",
    "-c:a",
    "libmp3lame",
    "-b:a",
    "32k",
    "-metadata",
    "artist=Stick",
    "-metadata",
    "album=Travel",
    "-metadata",
    `title=Song ${n}`,
    "-metadata",
    `track=${n}`,
    join(src, `s${n}.mp3`),
  ]);
const runKey = () =>
  spawnSync("reg", ["query", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run"], {
    encoding: "utf8",
  }).stdout;
const runBefore = runKey();
const env = {
  ...process.env,
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
};
delete env.SIRTONE_DATA_DIR;
const start = async (dir) => {
  const app = spawn(join(dir, "SirTone.exe"), [], { env, stdio: "ignore" });
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
  return { app, page, invoke };
};
let app = null;
try {
  check(
    "the portable folder is built",
    !!folder && existsSync(join(folder, "sirtone.portable")),
    folder,
  );
  let s = await start(folder);
  app = s.app;
  await s.invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await s.page.reload();
  await s.page.waitForSelector("footer");
  await s.invoke("import_paths", { paths: [join(work, "in", "Artist")] });
  await s.page.getByRole("button", { name: "Продолжить" }).click();
  await s.page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 60000 });
  const status = await s.invoke("library_status", {});
  const auto = await s.invoke("autostart_available", {});
  await sleep(2500);
  app.kill();
  await sleep(2000);
  const settings = JSON.parse(readFileSync(join(folder, "Data", "settings.json"), "utf8"));
  check(
    "data in .\\Data, music in .\\Music, the library path stored relative",
    existsSync(join(folder, "Data", "logs")) &&
      status.root?.startsWith(join(folder, "Music")) &&
      settings.library.root === "Music" &&
      existsSync(join(folder, "Music", "Stick", "Travel")),
    `${status.root} → «${settings.library.root}»`,
  );
  check("no autostart or updater in portable mode", auto === false, String(auto));
  // Another PC: the folder elsewhere (another drive letter).
  const moved = join(work, "elsewhere");
  renameSync(folder, moved);
  s = await start(moved);
  app = s.app;
  const status2 = await s.invoke("library_status", {});
  check(
    "moved to another place — the library is still there",
    status2.root?.startsWith(join(moved, "Music")) && status2.tracks === 2 && !status2.offline,
    `${status2.root} tracks ${status2.tracks}`,
  );
  check("nothing in the Windows Run key", runKey() === runBefore);
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await sleep(2000);
app?.kill();
console.log(failed ? "portable-quick: FAILED" : "portable-quick: ok");
process.exit(failed ? 1 : 0);
