// Stage 1 exit check on the APP (not only the CLI): kill sirtone-app at a random moment during an
// import, relaunch it (it recovers the journal and resumes the batch on its own), drop the same
// corpus again (must give only duplicates), then `sirtone-cli verify` must find 438 tracks and
// no problems. Isolated data dir per run — the owner's app and data are never touched.
//
//   node tools/app-kill-harness.mjs [runs=10]
// Needs target/release/sirtone-app.exe, target/release/sirtone-cli.exe, the import500 corpus.
import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const require = createRequire(join(root, "apps", "desktop", "package.json"));
const { chromium } = require("@playwright/test");
const APP = join(root, "target", "release", "sirtone-app.exe");
const CLI = join(root, "target", "release", "sirtone-cli.exe");
const CORPUS = join(root, "fixtures", "generated", "import500");
const RUNS = Number(process.argv[2] ?? 10);
const PORT = 9337;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let seed = 11;
const rand = () => {
  seed = (seed * 1103515245 + 12345) % 2 ** 31;
  return seed / 2 ** 31;
};

async function launch(env) {
  const proc = spawn(APP, [], { env, stdio: "ignore" });
  for (let i = 0; i < 80; i++) {
    try {
      const browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      const page = browser.contexts()[0]?.pages()[0];
      if (page) {
        await page.waitForSelector("footer");
        const invoke = (c, a = {}) =>
          page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [c, a]);
        return { proc, browser, invoke };
      }
      await browser.close();
    } catch {
      // not up yet
    }
    await sleep(250);
  }
  throw new Error("app did not start");
}

async function idle(invoke) {
  let calm = 0;
  for (let i = 0; i < 240 && calm < 3; i++) {
    const s = await invoke("library_status");
    calm = s.importing ? 0 : calm + 1;
    await sleep(250);
  }
  return invoke("library_status");
}

let failures = 0;
for (let run = 1; run <= RUNS; run++) {
  const data = mkdtempSync(join(tmpdir(), "sirtone-killapp-"));
  const env = {
    ...process.env,
    SIRTONE_DATA_DIR: data,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
  };
  const lib = join(data, "Music", "SirTone");
  let a = await launch(env);
  await a.invoke("library_set_root", { path: lib });
  await a.invoke("import_paths", { paths: [CORPUS] });
  const delay = 100 + Math.round(rand() * 1100);
  await sleep(delay);
  a.proc.kill();
  await a.browser.close().catch(() => {});
  await sleep(700);
  a = await launch(env); // recovers + resumes on start
  const resumed = await idle(a.invoke);
  await a.invoke("import_paths", { paths: [CORPUS] }); // the user drops it again
  const after = await idle(a.invoke);
  a.proc.kill();
  await a.browser.close().catch(() => {});
  await sleep(700);
  const v = spawnSync(CLI, ["verify", lib], { encoding: "utf8" });
  const ok = v.status === 0 && after.tracks === 438 && /438 tracks/.test(v.stdout);
  if (!ok) failures++;
  console.log(
    `run ${String(run).padStart(2)}: kill at ${delay} ms → after restart ${resumed.tracks}, after re-drop ${after.tracks}; ${v.stdout.trim()} → ${ok ? "OK" : "FAIL"}`,
  );
  rmSync(data, { recursive: true, force: true });
}
console.log(`\n${RUNS - failures}/${RUNS} app runs OK`);
process.exit(failures ? 1 : 0);
