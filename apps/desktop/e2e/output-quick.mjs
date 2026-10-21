// Output device (T5.8, FR-PLAY-012) in the real app, isolated: the devices are listed with
// names; choosing a device by id plays there (the snapshot names it) without stopping; «Как в
// Windows» goes back. Only the device that is already the Windows default is chosen, and
// exclusive mode is never switched on — nothing changes for the owner's sound.
// SMOKE_PORT=9363 node apps/desktop/e2e/output-quick.mjs

import { spawn, spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-outq-"));
const PORT = Number(process.env.SMOKE_PORT ?? 9363);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = false;
const check = (name, ok, detail = "") => {
  if (!ok) failed = true;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const dir = join(dataDir, "in", "Artist", "Album");
mkdirSync(dir, { recursive: true });
const tone = join(dir, "t.mp3");
spawnSync("ffmpeg", [
  "-y",
  "-v",
  "error",
  "-f",
  "lavfi",
  "-i",
  "sine=frequency=330:duration=60",
  "-af",
  "volume=-40dB",
  "-c:a",
  "libmp3lame",
  "-b:a",
  "32k",
  tone,
]);
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
  const devices = await invoke("audio_devices", {});
  const def = devices.find((d) => d.isDefault);
  check(
    "the output devices are listed by name, one is the Windows default",
    devices.length > 0 && !!def && devices.every((d) => d.name && d.id),
    devices.map((d) => `${d.name}${d.isDefault ? " *" : ""}`).join(", "),
  );
  await invoke("playback_open", { paths: [tone] });
  await sleep(2000);
  const before = await invoke("playback_snapshot", {});
  await invoke("settings_patch", {
    patch: { playback: { output: { device: def.id, exclusive: false } } },
  });
  await sleep(2000);
  const after = await invoke("playback_snapshot", {});
  check(
    "a device chosen by id: it plays on, at the same place",
    after.state === "playing" && after.positionMs >= before.positionMs && !after.signal?.exclusive,
    `${before.state}@${before.positionMs} → ${after.state}@${after.positionMs}`,
  );
  await invoke("settings_patch", { patch: { playback: { output: { device: "default" } } } });
  await sleep(1500);
  const back = await invoke("playback_snapshot", {});
  check("«Как в Windows» again — still playing", back.state === "playing", back.state);
  await invoke("playback_toggle", {});
} catch (e) {
  failed = true;
  console.log("FAIL ", e);
}
await sleep(2500);
app.kill();
console.log(failed ? "output-quick: FAILED" : "output-quick: ok");
process.exit(failed ? 1 : 0);
