// Export (T5.9, FR-DATA-003, AT-09 «Отдать другу») in the real app, isolated: an artist exported
// into a temp folder — the files are byte for byte the library's copies, `Артист/Альбом` kept;
// a second export of the same is skipped as already there; numbering in order; a folder inside
// the library is refused. Nothing outside the test's temp folders is touched.
// SMOKE_PORT=9364 node apps/desktop/e2e/export-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, statSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-expq-"));
const out = join(dataDir, "export");
const PORT = Number(process.env.SMOKE_PORT ?? 9364);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const src = join(dataDir, "in");
for (const [artist, album, n] of [
  ["Friend", "First", 3],
  ["Other", "Else", 2],
]) {
  const dir = join(src, artist, album);
  mkdirSync(dir, { recursive: true });
  for (let i = 0; i < n; i++)
    spawnSync("ffmpeg", [
      "-y",
      "-v",
      "error",
      "-f",
      "lavfi",
      "-i",
      `sine=frequency=${300 + i * 50}:duration=5`,
      "-c:a",
      "libmp3lame",
      "-b:a",
      "64k",
      "-metadata",
      `title=Song ${i + 1}`,
      "-metadata",
      `artist=${artist}`,
      "-metadata",
      `album=${album}`,
      "-metadata",
      `track=${i + 1}`,
      join(dir, `s${i}.mp3`),
    ]);
}
const files = (d) =>
  readdirSync(d, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? files(join(d, e.name)) : [join(d, e.name)],
  );
const md5 = (f) => createHash("md5").update(readFileSync(f)).digest("hex");
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
const finished = () =>
  page.evaluate(
    () =>
      new Promise((resolve) => {
        window.__TAURI_INTERNALS__
          .invoke("plugin:event|listen", {
            event: "export-progress",
            target: { kind: "Any" },
            handler: window.__TAURI_INTERNALS__.transformCallback((e) => {
              if (e.payload.finished) resolve(e.payload);
            }),
          })
          .catch(() => resolve(null));
      }),
  );
try {
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("import_paths", { paths: [join(src, "Friend"), join(src, "Other")] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 120000 });
  const status = await invoke("library_status", {});
  const libRoot = status.root;
  const artists = await invoke("collection_artists", { after: null, limit: 100 });
  const friend = artists.rows.find((a) => a.name === "Friend");
  const done1 = finished();
  const plan = await invoke("export_start", {
    req: { kind: "artist", id: friend.id, dest: out, keepStructure: true, number: false },
  });
  const p1 = await done1;
  const exported = files(out);
  const libFiles = files(join(libRoot, "Friend"));
  const sameBytes = exported.every((f) => {
    const twin = join(libRoot, relative(out, f));
    return existsSync(twin) && md5(twin) === md5(f);
  });
  check(
    "AT-09: the artist's files, byte for byte, «Артист/Альбом» kept",
    plan.files === 3 &&
      p1?.done === 3 &&
      exported.length === libFiles.length &&
      sameBytes &&
      exported.every((f) => relative(out, f).startsWith(join("Friend", "First"))),
    exported.map((f) => relative(out, f)).join(", "),
  );
  const done2 = finished();
  await invoke("export_start", {
    req: { kind: "artist", id: friend.id, dest: out, keepStructure: true, number: false },
  });
  const p2 = await done2;
  check(
    "the same export again — already there, nothing doubled",
    p2?.skipped === 3 && files(out).length === 3,
    JSON.stringify(p2),
  );
  const flat = join(dataDir, "flat");
  const done3 = finished();
  await invoke("export_start", {
    req: { kind: "all", id: null, dest: flat, keepStructure: false, number: true },
  });
  await done3;
  const names = readdirSync(flat).sort();
  check(
    "the whole library flat and numbered «01 - …»",
    names.length === 5 && names[0] === "01 - Song 1.mp3" && names[4].startsWith("05 - "),
    names.join(", "),
  );
  const inside = await page.evaluate(
    (d) =>
      window.__TAURI_INTERNALS__
        .invoke("export_start", {
          req: { kind: "all", id: null, dest: d, keepStructure: true, number: false },
        })
        .then(
          () => null,
          (e) => e,
        ),
    join(libRoot, "x"),
  );
  check(
    "a folder inside the library is refused",
    inside?.code === "InvalidArgument" && !existsSync(join(libRoot, "x")),
    JSON.stringify(inside),
  );
  check(
    "the library's files are untouched",
    libFiles.every((f) => statSync(f).size > 0) && files(join(libRoot, "Friend")).length === 3,
  );
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await sleep(2500);
app.kill();
console.log(failed ? "export-quick: FAILED" : "export-quick: ok");
process.exit(failed ? 1 : 0);
