// Real-app smoke on the built exe (release checklist). Not part of `pnpm e2e`: it drives the real
// app over WebView2 DevTools. The app runs ISOLATED (SIRTONE_DATA_DIR = fresh temp dir → own
// settings, logs, WebView2 profile and single-instance identity): the owner's running Сиртон and
// data are never touched, and nothing has to be closed first.
//
//   node apps/desktop/e2e/real-app.mjs [path\to\sirtone-app.exe]
//   (default exe: target\release\sirtone-app.exe; installed: %LOCALAPPDATA%\SirTone\sirtone-app.exe)
//
// Checks: command-line file plays (+ audio via loopback when the S-01 tool is built), Space,
// seek, broken file → toast, folder → first file, second launch forwards a file, theme and
// volume persist across a restart.
import { spawn, spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
} from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");

const root = join(import.meta.dirname, "..", "..", "..");
const exe = process.argv[2] ?? join(root, "target", "release", "sirtone-app.exe");
const fixtures = join(root, "fixtures", "generated");
const dataDir = mkdtempSync(join(tmpdir(), "sirtone-smoke-"));
// Isolated instances default the library folder into the data dir — never the owner's Music.
const libDir = join(dataDir, "Music", "SirTone");
const corpus = join(fixtures, "import500");
const PORT = Number(process.env.SMOKE_PORT ?? 9335);
const env = {
  ...process.env,
  SIRTONE_DATA_DIR: dataDir,
  WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
};

const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const toMs = (tc) => {
  const p = tc.split(":").map(Number);
  return (p.length === 3 ? p[0] * 3600 + p[1] * 60 + p[2] : p[0] * 60 + p[1]) * 1000;
};
const settings = () => JSON.parse(readFileSync(join(dataDir, "settings.json"), "utf8"));
// The listening journal of the isolated library (read-only; WAL lets the app keep writing).
const readJournal = () => {
  const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
  try {
    return db
      .prepare(
        "SELECT track_id, played_ms, end_reason, source, daypart, length(season_w) AS season_len FROM play_event ORDER BY id",
      )
      .all();
  } finally {
    db.close();
  }
};

function launch(args = []) {
  return spawn(exe, args, { env, detached: false, stdio: "ignore" });
}
async function connect() {
  for (let i = 0; i < 60; i++) {
    try {
      const b = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      const page = b.contexts()[0]?.pages()[0];
      if (page) {
        await page.waitForSelector("footer");
        return { browser: b, page };
      }
      await b.close();
    } catch {
      // not up yet
    }
    await sleep(250);
  }
  throw new Error("app did not expose DevTools");
}

let app = launch([join(fixtures, "long", "mix-3h.mp3")]);
let { browser, page } = await connect();
let footer = page.locator("footer");
// [elapsed, total] from the seek slider (the right-hand label shows the time left, D-75).
const times = async () =>
  ((await footer.locator("[role=slider]").first().getAttribute("aria-valuetext")) ?? "").split(
    " / ",
  );
const playLabel = () => footer.locator("button[aria-label]").nth(2).getAttribute("aria-label");
const invoke = (cmd, args) =>
  page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
// T2.16: axe on the real screens with data; critical violations are collected per screen.
const axePath = require.resolve("axe-core/axe.min.js", {
  paths: [dirname(require.resolve("@axe-core/playwright"))],
});
const axeCritical = [];
const axeScreen = async (name) => {
  // The app's CSP forbids injected <script>s (good); DevTools evaluation is not a page script.
  if (!(await page.evaluate(() => "axe" in window)))
    await page.evaluate(readFileSync(axePath, "utf8"));
  const ids = await page.evaluate(async () => {
    const r = await window.axe.run(document, { resultTypes: ["violations"] });
    return r.violations.filter((v) => v.impact === "critical").map((v) => v.id);
  });
  for (const id of ids) axeCritical.push(`${name}: ${id}`);
};

// 0. T2.15: a fresh install opens the first-start setup «1 из 4»; step through, skip the folder
// (chosen later by the library checks), then the tips.
const setup = page.getByRole("dialog", { name: "Первая настройка" });
const setupShown = await setup.waitFor({ timeout: 5000 }).then(
  () => true,
  () => false,
);
const stepLabel = setupShown ? await setup.getByText("1 из 4").count() : 0;
for (let i = 0; i < 3 && setupShown; i++) {
  await setup.getByRole("button", { name: "Далее" }).click();
}
const lastStep = setupShown ? await setup.getByText("4 из 4").count() : 0;
if (setupShown) await setup.getByRole("button", { name: "Пропустить" }).click();
const tipsOk = page.getByRole("button", { name: "Понятно" });
const tipsShown = await tipsOk.waitFor({ timeout: 3000 }).then(
  () => true,
  () => false,
);
if (tipsShown) await tipsOk.click();
// settings.json is written debounced (≤ 1 s): poll a little.
for (let i = 0; i < 15 && settings().onboarding?.done !== true; i++) await sleep(200);
check(
  "first start: setup «1 из 4» → «4 из 4», then the tips; saved as done",
  setupShown && stepLabel > 0 && lastStep > 0 && tipsShown && settings().onboarding?.done === true,
  `setup ${setupShown}, tips ${tipsShown}, done ${settings().onboarding?.done}`,
);

// 1. Command-line file plays — and is audible (process loopback, S-01 tool).
await sleep(1200);
const title = await footer.locator(".t-body-strong").first().textContent();
check("title from file name", title === "mix-3h", title);
const [a] = await times();
await sleep(1500);
const [b, total] = await times();
check("position advances while playing", toMs(b) > toMs(a), `${a} → ${b} of ${total}`);
check("play button shows pause", (await playLabel()) === "Пауза");
const loopback = join(root, "target", "spikes", "release", "s01-output.exe");
if (existsSync(loopback)) {
  const out = spawnSync(loopback, [
    "--capture-pid",
    String(app.pid),
    "--seconds",
    "2",
  ]).stdout.toString();
  const m = out.match(
    /signal ([\d.]+) s, RMS (-?[\d.]+) dBFS, 10 ms silent blocks inside signal: (\d+), discontinuity flags: (\d+)/,
  );
  check(
    "audible without gaps (loopback)",
    m && Number(m[1]) > 1.8 && Number(m[2]) > -80 && m[3] === "0" && m[4] === "0",
    m ? m[0] : out.trim(),
  );
}

// 1b. No dropped samples: the 440/660 Hz test tone (44.1 kHz → device rate) must reach the
//     device as an unbroken sine (S-01 click detector). Catches gaps that are not silence.
if (existsSync(loopback)) {
  await invoke("playback_open", { paths: [join(fixtures, "signals", "tone-10s.wav")] });
  await invoke("playback_set_volume", { volume: 0.3 });
  await sleep(800);
  const out = spawnSync(loopback, [
    "--capture-pid",
    String(app.pid),
    "--seconds",
    "3",
  ]).stdout.toString();
  const m = out.match(/clicks \(tone-10s\): L (\d+) \(worst ([\d.]+)\), R (\d+)/);
  check(
    "no clicks or dropped samples (resampled tone)",
    m && m[1] === "0" && m[3] === "0",
    m ? m[0] : out.trim(),
  );
  await invoke("playback_open", { paths: [join(fixtures, "long", "mix-3h.mp3")] });
  await sleep(800);
}

// 1c. Gapless (T1.5): two halves of one 1 kHz sine played back to back through the engine
//     (sirtone-cli) must join without a gap or a click.
const cli = join(root, "target", "release", "sirtone-cli.exe");
const halves = ["gapless-a.wav", "gapless-b.wav"].map((f) => join(fixtures, "signals", f));
if (existsSync(loopback) && existsSync(cli) && halves.every((h) => existsSync(h))) {
  const player = spawn(cli, ["play", ...halves, "--volume", "0.3"], { stdio: "ignore" });
  await sleep(300);
  const out = spawnSync(loopback, [
    "--capture-pid",
    String(player.pid),
    "--seconds",
    "7",
    "--sine-hz",
    "1000",
  ]).stdout.toString();
  player.kill();
  const m = out.match(/clicks \(1000 Hz\): L (\d+) \(worst ([\d.]+)\), R (\d+)/);
  const sig = out.match(/signal ([\d.]+) s/);
  check(
    "gapless joint (WAV halves)",
    m && m[1] === "0" && m[3] === "0" && Number(m[2]) < 0.005 && sig && Number(sig[1]) > 6.5,
    m ? `${m[0]}; ${sig?.[0]}` : out.trim(),
  );
}

// 1d. Gapless across formats: a 44.1 kHz half then a 48 kHz half (second deck path).
const mixed = ["gapless-a.wav", "gapless-b48.wav"].map((f) => join(fixtures, "signals", f));
if (existsSync(loopback) && existsSync(cli) && mixed.every((h) => existsSync(h))) {
  let printed = "";
  const player = spawn(cli, ["play", ...mixed, "--volume", "0.3"]);
  player.stdout.on("data", (d) => {
    printed += d.toString();
  });
  await sleep(300);
  const out = spawnSync(loopback, [
    "--capture-pid",
    String(player.pid),
    "--seconds",
    "7",
    "--sine-hz",
    "1000",
  ]).stdout.toString();
  await sleep(1500);
  player.kill();
  const m = out.match(/clicks \(1000 Hz\): L (\d+) \(worst ([\d.]+)\)/);
  const sig = out.match(/signal ([\d.]+) s/);
  check(
    "gapless joint across sample rates",
    m &&
      Number(m[2]) < 0.05 &&
      sig &&
      Number(sig[1]) > 6.5 &&
      /gapless-b48/.test(printed) &&
      /underruns 0\s*$/.test(printed.trim()),
    m ? `${m[0]}; ${sig?.[0]}; switched ${/gapless-b48/.test(printed)}` : out.trim(),
  );
}

// 2. Space toggles.
await page.evaluate(() => document.activeElement?.blur());
await page.keyboard.press("Space");
await sleep(400);
check("space pauses", (await playLabel()) === "Слушать");
const [p1] = await times();
await sleep(1200);
const [p2] = await times();
check("position frozen while paused", p1 === p2, `${p1} / ${p2}`);
await page.keyboard.press("Space");
await sleep(400);
check("space resumes", (await playLabel()) === "Пауза");

// 3. Seek to the middle by clicking the progress bar.
const bar = footer.getByRole("slider", { name: "Позиция в треке" });
const box = await bar.boundingBox();
await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
await sleep(600);
const [s, dur] = await times();
check("seek to middle", Math.abs(toMs(s) / toMs(dur) - 0.5) < 0.02, `${s} of ${dur}`);

// 4. Broken file → toast, current track keeps playing.
await invoke("playback_open", { paths: [join(fixtures, "broken", "text-as.mp3")] });
// dnd-kit keeps its own (empty) live region with role=status: take the toast by its text.
const toast = page.getByRole("status").filter({ hasText: "text-as.mp3" });
await toast.waitFor({ timeout: 3000 }).catch(() => {});
const toastText = (await toast.count()) ? await toast.textContent() : "";
check(
  "error toast for broken file",
  toastText.includes("не поддерживается") && toastText.includes("text-as.mp3"),
  toastText,
);
check("still playing after error", (await playLabel()) === "Пауза");

// 5. Space still toggles after the volume slider was touched (no blur).
await footer.getByRole("slider", { name: "Громкость" }).click();
await page.keyboard.press("Space");
await sleep(400);
check("space works after slider focus", (await playLabel()) === "Слушать");
await page.keyboard.press("Space");
await sleep(400);

// 6. A folder plays its first playable file.
await invoke("playback_open", { paths: [join(fixtures, "formats")] });
await sleep(800);
const folderTitle = await footer.locator(".t-body-strong").first().textContent();
check("folder plays first file", folderTitle === "aac-256", folderTitle);

// 7. Second launch with a file («Открыть с помощью» while running) → forwarded, plays.
launch([join(fixtures, "formats", "flac-16-44.flac")]);
await sleep(2000);
const t2 = await footer.locator(".t-body-strong").first().textContent();
check("second launch forwards the file", t2 === "flac-16-44", t2);

// 9. Library (Stage 1): import without a folder → «Папка для музыки»; folder set → import runs,
//    report; Albums → album → double click plays the context; «вперёд» moves on.
if (existsSync(corpus)) {
  const start = await invoke("import_paths", { paths: [corpus] });
  check("import without a folder asks for one", start === "rootNeeded", String(start));
  await page.getByRole("heading", { name: "Папка для музыки" }).waitFor({ timeout: 3000 });
  check("folder panel shown", true);
  // The real button: default folder → «Продолжить» → panel closes → the waiting import starts.
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("heading", { name: "Папка для музыки" })
    .waitFor({ state: "detached", timeout: 5000 });
  check("«Продолжить» closes the panel", true);
  const report = page.getByRole("status").filter({ hasText: "Музыка добавлена" });
  await report.waitFor({ timeout: 60000 });
  const text = await report.textContent();
  check(
    "import report",
    /Добавлено 438/.test(text) && /Дубли 10/.test(text) && /Не добавлено 52/.test(text),
    text,
  );
  const st = await invoke("library_status", {});
  check(
    "library counters",
    st.tracks === 438 && st.albums > 30,
    `${st.tracks} tracks, ${st.albums} albums`,
  );
  await page.getByRole("button", { name: "Альбомы" }).click();
  await page.getByText(/Вся коллекция: 438 треков/).waitFor({ timeout: 5000 });
  check("collection summary", true);
  // Optional screenshots for a human look (SMOKE_SHOTS=<dir>).
  if (process.env.SMOKE_SHOTS) {
    await sleep(1200);
    await page.screenshot({ path: join(process.env.SMOKE_SHOTS, "albums.png") });
    await page.getByRole("button", { name: "Исполнители" }).click();
    await sleep(1000);
    await page.screenshot({ path: join(process.env.SMOKE_SHOTS, "artists.png") });
    await page.getByRole("button", { name: "Альбомы" }).click();
    await sleep(800);
  }
  // Covers (T1.26): album rows show WebP from sirtone-cover:// that really decode.
  await sleep(800);
  const covers = await page.evaluate(() =>
    [...document.querySelectorAll('main img[src^="http://sirtone-cover.localhost/"]')]
      .filter((i) => i.getBoundingClientRect().bottom < window.innerHeight) // lazy: on-screen only
      .map((i) => i.complete && i.naturalWidth > 0),
  );
  check(
    "album covers load",
    covers.length >= 8 && covers.every(Boolean),
    `${covers.filter(Boolean).length}/${covers.length}`,
  );
  await page.locator("main ul li button").first().click();
  const rows = page.getByRole("list", { name: "Треки" }).locator("button[aria-pressed]");
  await rows.first().waitFor({ timeout: 5000 });
  const firstTitle = (await rows.first().locator(".t-body").textContent())?.trim();
  await rows.first().dblclick();
  await sleep(1200);
  const playing = await footer.locator(".t-body-strong").first().textContent();
  check("double click plays the track", playing === firstTitle, `${playing} / ${firstTitle}`);
  // T1.23: Windows media controls (flyout, media keys, widgets) show the track and their ⏯
  // reaches the player — on this isolated instance only (matched by the track title).
  const smtcList = spawnSync(cli, ["smtc"], { encoding: "utf8" }).stdout;
  check("media controls show the track", smtcList.includes(playing), smtcList.trim());
  spawnSync(cli, ["smtc", "toggle", playing], { encoding: "utf8" });
  await sleep(600);
  const paused = await invoke("playback_snapshot", {});
  check("media-controls ⏯ pauses", paused.state === "paused", paused.state);
  spawnSync(cli, ["smtc", "toggle", playing], { encoding: "utf8" });
  await sleep(600);
  // The album continues by itself (gapless handoff from the queue): no button pressed.
  const auto = await page
    .waitForFunction(
      (t) => document.querySelector("footer .t-body-strong")?.textContent !== t,
      playing,
      { timeout: 8000 },
    )
    .then(
      () => true,
      () => false,
    );
  const snapAfter = await invoke("playback_snapshot", {});
  check(
    "album plays on by itself, no underruns",
    auto && snapAfter.underruns === 0 && snapAfter.state === "playing",
    `${snapAfter.title}, underruns ${snapAfter.underruns}`,
  );
  const nextBtn = footer.getByRole("button", { name: "Вперёд" });
  check("next enabled in an album", await nextBtn.isEnabled());
  await nextBtn.click();
  await sleep(1200);
  const second = await footer.locator(".t-body-strong").first().textContent();
  check("next plays the following track", second !== playing, `${playing} → ${second}`);
  // T2.11: track menu → «Воспроизвести далее» puts that track right after the current one.
  const lastRow = rows.last();
  const lastTitle = (await lastRow.locator(".t-body").textContent())?.trim();
  await lastRow.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Воспроизвести далее" }).click();
  await sleep(500);
  const qv = await invoke("queue_get", {});
  check(
    "«Воспроизвести далее» queues the track next",
    qv.items[qv.position + 1]?.title === lastTitle,
    `${qv.items[qv.position + 1]?.title} / ${lastTitle}`,
  );
  await footer.getByRole("button", { name: "Очередь" }).click();
  const panel = page.getByRole("complementary", { name: "Очередь" });
  await panel.waitFor({ timeout: 3000 });
  const listed = await panel
    .getByText(lastTitle, { exact: true })
    .first()
    .waitFor({ timeout: 3000 })
    .then(
      () => true,
      () => false,
    );
  check("queue panel lists the queue", listed);
  await footer.getByRole("button", { name: "Очередь" }).click();
  // FR-PLAY-007: repeat cycles off → all → one; shuffle on/off.
  await footer.getByRole("button", { name: "Повтор" }).click();
  await sleep(300);
  const r1 = (await invoke("playback_snapshot", {})).repeat;
  await footer.getByRole("button", { name: "Повторять всё" }).click();
  await sleep(300);
  const r2 = (await invoke("playback_snapshot", {})).repeat;
  await footer.getByRole("button", { name: "Повторять трек" }).click();
  await sleep(300);
  const r3 = (await invoke("playback_snapshot", {})).repeat;
  check(
    "repeat cycles off → all → one → off",
    r1 === "all" && r2 === "one" && r3 === "off",
    `${r1} ${r2} ${r3}`,
  );
  await footer.getByRole("button", { name: "Перемешать" }).click();
  await sleep(400);
  // Snapshot and queue are two calls: a track may end in between, so take a consistent pair.
  let sh;
  let q2;
  for (let i = 0; i < 3; i++) {
    sh = await invoke("playback_snapshot", {});
    q2 = await invoke("queue_get", {});
    if (sh.title === (await invoke("playback_snapshot", {})).title) break;
  }
  check(
    "shuffle on keeps the playing track",
    sh.shuffle === true && q2.items[q2.position]?.title === sh.title,
    `shuffle ${sh.shuffle}, queue[${q2.position}] ${q2.items[q2.position]?.title} / ${sh.title}`,
  );
  await footer.getByRole("button", { name: "Перемешать" }).click();
  // T2.1: the track menu «К исполнителю» opens the artist; Alt+← goes back to the album.
  const albumTitle = await page.locator("h1").first().textContent();
  await rows.first().click({ button: "right" });
  await page.getByRole("menuitem", { name: "К альбому / исполнителю" }).click();
  await page.getByRole("menuitem", { name: "К исполнителю" }).click();
  await sleep(600);
  const artistTitle = await page.locator("h1").first().textContent();
  await page.keyboard.press("Alt+ArrowLeft");
  await sleep(400);
  const backTitle = await page.locator("h1").first().textContent();
  check(
    "track menu → artist, Alt+← → back to the album",
    artistTitle !== albumTitle && backTitle === albumTitle,
    `${albumTitle} → ${artistTitle} → ${backTitle}`,
  );
  // T2.10: «Сохранить» on an unchanged card of an identified track moves nothing.
  await rows.nth(1).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Ещё" }).click();
  await page.getByRole("menuitem", { name: "Изменить данные" }).click();
  const albumCard = page.getByRole("dialog");
  const pathBefore =
    (await albumCard.locator("dd.break-all").textContent({ timeout: 5000 }))?.trim() ?? "";
  await albumCard.getByRole("button", { name: "Сохранить" }).click();
  await albumCard.waitFor({ state: "detached", timeout: 5000 }).catch(() => {});
  await rows.nth(1).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Ещё" }).click();
  await page.getByRole("menuitem", { name: "Изменить данные" }).click();
  const pathAfter =
    (
      await page.getByRole("dialog").locator("dd.break-all").textContent({ timeout: 5000 })
    )?.trim() ?? "";
  await page.keyboard.press("Escape");
  check(
    "unchanged track card keeps the file where it is",
    pathBefore !== "" && pathAfter === pathBefore && existsSync(pathBefore),
    `${pathBefore} → ${pathAfter}`,
  );
  // T1.31: the journal — the first track was heard to its end, the second was skipped.
  await sleep(1500);
  const journal = readJournal();
  const finished = journal.find((r) => r.end_reason === "finished" && r.source === "album");
  check(
    "journal: album track finished with its context",
    !!finished && finished.season_len === 4 && finished.daypart >= 0 && finished.played_ms > 1000,
    JSON.stringify(finished),
  );
  check(
    "journal: «next» is a skip",
    journal.some((r) => r.end_reason === "skip"),
    journal.map((r) => r.end_reason).join(","),
  );
  const attrs = spawnSync("powershell", [
    "-NoProfile",
    "-Command",
    `(Get-Item -Force '${join(libDir, ".sirtone")}').Attributes`,
  ]).stdout.toString();
  check("service folder is hidden", /Hidden/.test(attrs), attrs.trim());
  await page.getByRole("button", { name: "Неизвестные" }).click();
  await page.getByText(/35 треков/).waitFor({ timeout: 5000 });
  const unknownRows = page.getByRole("list", { name: "Треки" }).locator("button[aria-pressed]");
  await unknownRows.first().dblclick();
  await sleep(1000);
  const unknownTitle = (await unknownRows.first().locator(".t-body").textContent())?.trim();
  const nowPlaying = await footer.locator(".t-body-strong").first().textContent();
  check("«Неизвестные» list plays", nowPlaying === unknownTitle, `${nowPlaying} / ${unknownTitle}`);
  // T2.9: pick an unknown track, say who it is — it leaves the list, its file moves.
  const before = (await invoke("library_status", {})).unknown;
  await unknownRows.nth(1).click();
  const assignPanel = page.getByRole("complementary", { name: "Назначить" });
  await assignPanel.getByLabel("Название").fill("Проверочная песня");
  await assignPanel.getByLabel("Исполнитель").fill("Проверочный исполнитель");
  await assignPanel.getByRole("button", { name: "Назначить" }).click();
  await sleep(1500);
  const after = (await invoke("library_status", {})).unknown;
  const movedDir = join(libDir, "Проверочный исполнитель");
  check(
    "assigning an unknown track moves it out",
    after === before - 1 && existsSync(movedDir),
    `${before} → ${after}, folder ${existsSync(movedDir)}`,
  );
  // T2.10: track card through «Ещё ›», then «только убрать из приложения» keeps the file.
  await unknownRows.nth(2).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Ещё" }).click();
  await page.getByRole("menuitem", { name: "Изменить данные" }).click();
  const card = page.getByRole("dialog");
  const cardPath =
    (await card.locator("dd.break-all").textContent({ timeout: 5000 }))?.trim() ?? "";
  check("track card shows the file", existsSync(cardPath), cardPath);
  await page.keyboard.press("Escape");
  check("Esc closes the track card", (await page.getByRole("dialog").count()) === 0);
  await unknownRows.nth(2).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Удалить из медиатеки" }).click();
  await page.getByRole("button", { name: "Только убрать из приложения" }).click();
  await sleep(1500);
  const afterRemove = (await invoke("library_status", {})).unknown;
  check(
    "«только из приложения» forgets the track, keeps the file",
    afterRemove === after - 1 && existsSync(cardPath),
    `${after} → ${afterRemove}, file ${existsSync(cardPath)}`,
  );
  // FR-DATA-002: deleting an artist asks with the count and size; «только из приложения».
  const artistsBefore = (await invoke("library_status", {})).artists;
  await page.getByRole("button", { name: "Исполнители" }).click();
  const artistCard = page.getByRole("list", { name: "Исполнители" }).getByRole("button").first();
  await artistCard.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Удалить исполнителя" }).click();
  const question = await page.getByRole("dialog").locator("p").textContent({ timeout: 5000 });
  check("artist deletion shows count and size", /\d+ трек.*МБ/.test(question ?? ""), question);
  await page.getByRole("button", { name: "Только убрать из приложения" }).click();
  await sleep(2000);
  const artistsAfter = (await invoke("library_status", {})).artists;
  check(
    "artist removed from the app",
    artistsAfter < artistsBefore,
    `${artistsBefore} → ${artistsAfter}`,
  );
  // T2.11: new playlist from the sidebar, named in place; a track via «Добавить в плейлист ›»;
  // «Убрать из плейлиста» keeps the track in the library (FR-PLS-004).
  await page.getByRole("button", { name: "Новый плейлист" }).click();
  const nameField = page.getByLabel("Название плейлиста");
  await nameField.waitFor({ timeout: 5000 });
  await nameField.fill("Дорога");
  await nameField.press("Enter");
  await page.getByRole("button", { name: "Неизвестные" }).click();
  const firstUnknown = page
    .getByRole("list", { name: "Треки" })
    .locator("button[aria-pressed]")
    .first();
  const plTitle = (await firstUnknown.locator(".t-body").textContent())?.trim();
  await firstUnknown.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Добавить в плейлист" }).click();
  await page.getByRole("menuitem", { name: "Дорога" }).click();
  await sleep(800);
  await page.getByRole("navigation").getByRole("button", { name: "Дорога" }).click();
  const plRows = page.getByRole("list", { name: "Треки" }).locator("button[aria-pressed]");
  await plRows.first().waitFor({ timeout: 5000 });
  const plListed = (await plRows.first().locator(".t-body").textContent())?.trim();
  check(
    "playlist: created, named and the track added",
    plListed === plTitle,
    `${plListed} / ${plTitle}`,
  );
  const tracksBefore = (await invoke("library_status", {})).tracks;
  await plRows.first().click({ button: "right" });
  await page.getByRole("menuitem", { name: "Убрать из плейлиста" }).click();
  await sleep(800);
  const tracksAfter = (await invoke("library_status", {})).tracks;
  check(
    "«Убрать из плейлиста» keeps the track in the library",
    (await plRows.count()) === 0 && tracksAfter === tracksBefore,
    `rows ${await plRows.count()}, tracks ${tracksBefore} → ${tracksAfter}`,
  );
  // T2.8: pointer drag — a track onto a sidebar playlist, reorder inside it, spring-loaded
  // «Исполнители» and a drop onto an artist card (assign, confirmed).
  const drag = async (from, to, holdMs = 0) => {
    const a = await from.boundingBox();
    const b = await to.boundingBox();
    await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
    await page.mouse.down();
    await page.mouse.move(a.x + a.width / 2 + 12, a.y + a.height / 2, { steps: 4 });
    await page.mouse.move(b.x + b.width / 2, b.y + Math.min(8, b.height / 2), { steps: 12 });
    if (holdMs) await sleep(holdMs);
    await page.mouse.up();
  };
  const road = (await invoke("playlists_list", {})).find((p) => p.name === "Дорога");
  await page.getByRole("button", { name: "Неизвестные" }).click();
  const uRows = page.getByRole("list", { name: "Треки" }).locator("li");
  await uRows.first().waitFor({ timeout: 5000 });
  const sideRoad = page.getByRole("navigation").getByRole("button", { name: "Дорога" });
  await drag(uRows.nth(0), sideRoad);
  await sleep(800);
  await drag(uRows.nth(1), sideRoad);
  await sleep(800);
  const afterDrop = await invoke("playlist_tracks", { id: road.id });
  check(
    "drag a track onto a sidebar playlist adds it",
    afterDrop.length === 2,
    `${afterDrop.length} items`,
  );
  await sideRoad.click();
  const pRows = page.getByRole("list", { name: "Треки" }).locator("li");
  await pRows.nth(1).waitFor({ timeout: 5000 });
  await drag(pRows.nth(1), pRows.nth(0));
  await sleep(800);
  const reordered = await invoke("playlist_tracks", { id: road.id });
  check(
    "drag reorders the playlist",
    reordered[0]?.track.id === afterDrop[1]?.track.id &&
      reordered[1]?.track.id === afterDrop[0]?.track.id,
    reordered.map((e) => e.track.title).join(", "),
  );
  const unknownBefore = (await invoke("library_status", {})).unknown;
  await page.getByRole("button", { name: "Неизвестные" }).click();
  await uRows.first().waitFor({ timeout: 5000 });
  const dragged = (await uRows.first().locator(".t-body").first().textContent())?.trim();
  // Spring-loading: hold over «Исполнители» until it opens, then drop onto the first card.
  const rowBox = await uRows.first().boundingBox();
  await page.mouse.move(rowBox.x + rowBox.width / 2, rowBox.y + rowBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(rowBox.x + rowBox.width / 2 + 12, rowBox.y + rowBox.height / 2, {
    steps: 4,
  });
  const navArtists = await page
    .getByRole("navigation")
    .getByRole("button", { name: "Исполнители" })
    .boundingBox();
  await page.mouse.move(navArtists.x + 40, navArtists.y + navArtists.height / 2, { steps: 10 });
  await sleep(1300);
  const firstArtist = page.getByRole("list", { name: "Исполнители" }).getByRole("button").first();
  const opened = await firstArtist.waitFor({ timeout: 3000 }).then(
    () => true,
    () => false,
  );
  if (opened) {
    const cardBox = await firstArtist.boundingBox();
    await page.mouse.move(cardBox.x + cardBox.width / 2, cardBox.y + cardBox.height / 2, {
      steps: 10,
    });
  }
  await page.mouse.up();
  const confirm = page.getByRole("button", { name: "Отнести" });
  const asked = await confirm.waitFor({ timeout: 3000 }).then(
    () => true,
    () => false,
  );
  const assignQuestion = asked ? await page.getByRole("dialog").locator("p").textContent() : "";
  const consoleErrors = [];
  page.on("console", (m) => {
    if (m.type() === "error") consoleErrors.push(m.text());
  });
  if (asked) {
    await confirm.click();
    // The dialog flies in (150 ms): a click during the animation can miss — once more if so.
    await page
      .getByRole("dialog")
      .waitFor({ state: "detached", timeout: 2000 })
      .catch(() => confirm.click().catch(() => {}));
  }
  await sleep(1500);
  const assignToast = (await page.getByRole("status").allTextContents()).join(" | ");
  const unknownAfter = (await invoke("library_status", {})).unknown;
  check(
    "spring-loaded «Исполнители» + drop on an artist assigns the track",
    opened && asked && unknownAfter === unknownBefore - 1,
    `${dragged}: opened ${opened}, asked ${asked}, unknown ${unknownBefore} → ${unknownAfter}; ${assignToast}; ${assignQuestion}; ${consoleErrors.join(" ")}`,
  );
  // T2.13: Ctrl+F opens search; results as you type, the exact artist is the best match.
  await page.keyboard.press("Control+f");
  const searchField = page.getByLabel("Поиск по медиатеке");
  await searchField.waitFor({ timeout: 3000 });
  await searchField.fill("ленин");
  const bestCard = page.getByRole("button", { name: "Ленинград", exact: true }).first();
  const found = await bestCard.waitFor({ timeout: 3000 }).then(
    () => true,
    () => false,
  );
  check("Ctrl+F search finds the artist as you type", found);
  await axeScreen("search results");
  const times = [];
  for (const q of ["ленинград", "dream", "мо", "плоковник"]) {
    const t0 = performance.now();
    await invoke("library_search", { query: q });
    times.push(performance.now() - t0);
  }
  const exact = await invoke("library_search", { query: "ленинград" });
  check(
    "search: exact artist is the best match, IPC round trip ≤ 80 ms",
    exact.best?.kind === "artist" && Math.max(...times) <= 80,
    `${exact.best?.kind}, max ${Math.max(...times).toFixed(1)} ms`,
  );
  // T2.7: sort albums «По году» — the newest album comes first.
  await page.getByRole("navigation").getByRole("button", { name: "Альбомы" }).click();
  await page.getByRole("button", { name: "По названию" }).click();
  await page.getByRole("menuitem", { name: "По году" }).click();
  const allAlbums = (await invoke("collection_albums", { after: null, limit: 500 })).rows;
  const newest = Math.max(...allAlbums.map((a) => a.year ?? 0));
  const firstSub = await page
    .getByRole("list", { name: "Альбомы" })
    .locator(".t-caption")
    .first()
    .textContent();
  check(
    "albums sort «По году» puts the newest first",
    firstSub?.endsWith(String(newest)),
    `${firstSub} / ${newest}`,
  );
  await page.getByRole("button", { name: "По году" }).click();
  await page.getByRole("menuitem", { name: "По названию" }).click();
  // T2.7: «Жанры» from tags — «Rock» and «rock» are one genre; its page lists its tracks.
  await page.getByRole("navigation").getByRole("button", { name: "Жанры" }).click();
  const genreList = page.getByRole("list", { name: "Жанры" });
  await genreList.waitFor({ timeout: 5000 });
  const tiles = await genreList.locator(".t-body-strong").allTextContents();
  check(
    "genres merge «Rock» and «rock»",
    tiles.includes("Rock") && !tiles.includes("rock") && tiles.length === 4,
    tiles.join(", "),
  );
  await genreList.getByText("Rock", { exact: true }).click();
  const genreRows = page.getByRole("list", { name: "Треки" }).locator("button[aria-pressed]");
  await genreRows.first().waitFor({ timeout: 5000 });
  check("genre page lists its tracks", (await page.locator("h1").first().textContent()) === "Rock");
  await axeScreen("genre page");
  // T2.5 / T2.19: «Моя волна» is the player — like, «не сейчас» skips, «никогда» for the artist
  // lands in Settings and comes back from there.
  await genreRows.first().dblclick();
  await sleep(800);
  await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
  const heart = page.getByRole("button", { name: "Нравится" });
  await heart.click();
  await sleep(500);
  const nowId = (await invoke("playback_snapshot", {})).trackId;
  const fbState = await invoke("track_feedback", { track: nowId });
  check(
    "wave: ♡ likes the playing track",
    fbState.liked && (await heart.getAttribute("aria-pressed")) === "true",
  );
  await page.getByRole("button", { name: "Не сейчас" }).click();
  await sleep(1200);
  const afterSkip = (await invoke("playback_snapshot", {})).trackId;
  check("wave: «Не сейчас» goes to the next track", afterSkip !== nowId, `${nowId} → ${afterSkip}`);
  await axeScreen("«Моя волна»");
  await page.getByRole("main").getByRole("button", { name: "Ещё" }).click();
  await page.getByRole("menuitem", { name: "Никогда в волне" }).click();
  await page.getByRole("menuitem", { name: "Этого исполнителя" }).click();
  await sleep(600);
  await page.getByRole("navigation").getByRole("button", { name: "Настройки" }).click();
  const backBtn = page.getByRole("button", { name: "Вернуть", exact: true });
  const neverListed = await backBtn
    .first()
    .waitFor({ timeout: 3000 })
    .then(
      () => true,
      () => false,
    );
  if (neverListed) await backBtn.first().click();
  await sleep(600);
  const neverLeft = await invoke("never_list", {});
  check(
    "«Никогда» is listed in Settings and «Вернуть» lifts it",
    neverListed && neverLeft.length === 0,
    `listed ${neverListed}, left ${neverLeft.length}`,
  );
  // T2.14: «Хранилище» shows the service data next to the library.
  const storageInfo = await invoke("storage_info", {});
  check(
    "storage shows the database and the cover cache",
    (await page.getByText("База медиатеки").count()) > 0 &&
      storageInfo.databaseBytes > 0 &&
      storageInfo.coversFiles > 0,
    `db ${storageInfo.databaseBytes} B, covers ${storageInfo.coversFiles}`,
  );
  // T2.2: «Авто» — the playing cover picks one of the sets (DESIGN.md §4), saved as «auto».
  await page.getByRole("button", { name: "Авто", exact: true }).click();
  let autoAccent = "";
  for (let i = 0; i < 15 && !autoAccent; i++) {
    await sleep(200);
    const a = await page.evaluate(() => document.documentElement.dataset.accent ?? "");
    if (a && a !== "custom") autoAccent = a;
  }
  for (let i = 0; i < 15 && settings().appearance?.accent?.source !== "auto"; i++) await sleep(200);
  check(
    "«Авто» picks a set from the cover and is saved",
    autoAccent !== "" && settings().appearance?.accent?.source === "auto",
    `${autoAccent}, cover ${(await invoke("playback_snapshot", {})).cover}`,
  );
  await axeScreen("settings");
  check(
    "axe: no critical violations on screens with data",
    axeCritical.length === 0,
    axeCritical.join("; ") || "none",
  );
  // T2.4: auto-detect — hardware + WebGL benchmark → a level, saved like a manual choice.
  await page.getByRole("navigation").getByRole("button", { name: "Настройки" }).click();
  await page.getByRole("button", { name: "Определить автоматически" }).click();
  const detected = await page
    .getByText(/^Определено: /)
    .first()
    .textContent({ timeout: 10000 })
    .catch(() => null);
  await sleep(1500); // debounced settings write
  const perfLevel = settings().performance?.level;
  check(
    "performance auto-detect picks and saves a level",
    detected !== null && Number.isInteger(perfLevel),
    `${detected?.trim()} (level ${perfLevel})`,
  );
  await page.getByRole("button", { name: "Настройки" }).click();
  check("storage shows the folder", (await page.getByText(libDir).count()) > 0);
} else {
  console.log("SKIP  library checks (run tools/fixtures/make-import-corpus.py)");
}

// 8. Theme and volume persist through Rust settings (T0.6/T0.7, D-64) and survive a restart.
await page.getByRole("button", { name: "Настройки" }).click();
await page.getByRole("button", { name: "Светлая" }).click();
await invoke("playback_set_volume", { volume: 0.37 });
await sleep(1600); // debounced write ≤ 1 s
check("theme saved", settings().appearance.theme === "light");
check("volume saved", Math.abs(settings().playback.volume - 0.37) < 1e-6);
await browser.close();
app.kill();
await sleep(800);
app = launch();
({ browser, page } = await connect());
footer = page.locator("footer");
await sleep(800);
check("theme after restart", (await page.locator("html").getAttribute("data-theme")) === "light");
const vol = await footer.getByRole("slider", { name: "Громкость" }).getAttribute("aria-valuenow");
check("volume after restart", vol === "37", vol);
if (existsSync(join(libDir, ".sirtone", "library.db"))) {
  check("journal survives a kill", readJournal().length >= 2);
}

// 9. Watching the folder (T1.19–T1.20): Explorer additions, deletions, an offline root.
if (existsSync(join(libDir, ".sirtone", "library.db"))) {
  const tracks = async () => (await invoke("library_status", {})).tracks;
  const waitTracks = async (want, ms) => {
    const t0 = Date.now();
    let n = await tracks();
    while (n !== want && Date.now() - t0 < ms) {
      await sleep(100);
      n = await tracks();
    }
    return [n, Date.now() - t0];
  };
  const before = await tracks();
  mkdirSync(join(libDir, "Снаружи"), { recursive: true });
  const added = join(libDir, "Снаружи", "Искала.flac");
  copyFileSync(join(root, "fixtures", "generated", "tags", "cyrillic-ok.flac"), added);
  const [n1, ms1] = await waitTracks(before + 1, 5000);
  check("file added in Explorer appears ≤ 2 s", n1 === before + 1 && ms1 <= 2000, `${ms1} ms`);
  rmSync(added);
  const [n2, ms2] = await waitTracks(before, 5000);
  check("deleted file leaves the lists", n2 === before, `${ms2} ms`);
  // Restart with the folder renamed: offline, nothing recreated at the old path.
  await browser.close();
  app.kill();
  await sleep(1000);
  const away = `${libDir}-away`;
  renameSync(libDir, away);
  app = launch();
  ({ browser, page } = await connect());
  await sleep(800);
  const st = await invoke("library_status", {});
  check("renamed folder → offline", st.offline === true, JSON.stringify({ offline: st.offline }));
  check("no new library at the old path", !existsSync(libDir));
  renameSync(away, libDir);
  const t0 = Date.now();
  let back = await invoke("library_status", {});
  while ((back.offline || back.tracks !== before) && Date.now() - t0 < 10000) {
    await sleep(250);
    back = await invoke("library_status", {});
  }
  check(
    "folder back → online, all tracks there",
    !back.offline && back.tracks === before,
    `${Date.now() - t0} ms, ${back.tracks}/${before}`,
  );
}

// T1.22: five default hotkeys configured; each either registered or reported as taken (when the
// owner's own Сиртон holds them, this isolated copy must see a conflict, not fail).
const hk = await invoke("hotkeys_status", {});
check(
  "hotkeys configured, conflicts reported",
  hk.length === 5 && hk.every((h) => typeof h.registered === "boolean"),
  hk.map((h) => `${h.accel}:${h.registered ? "ok" : "taken"}`).join(" "),
);

// T2.14 combination editor: click the combination, press a new one — registered at once.
await page.getByRole("button", { name: "Настройки" }).click();
await page.getByRole("button", { name: "Изменить сочетание: Играть / пауза" }).click();
await page.keyboard.press("Control+Alt+KeyP");
await sleep(500);
const edited = (await invoke("hotkeys_status", {})).find((h) => h.action === "playPause");
check("combination editor sets Ctrl+Alt+P", edited?.accel === "Ctrl+Alt+P", JSON.stringify(edited));
await invoke("settings_patch", { patch: { hotkeys: { play_pause: "Ctrl+Alt+Space" } } });

// FR-WIN-001: changing a hotkey applies at once (re-registration from a settings command).
await invoke("settings_patch", { patch: { hotkeys: { mute: "Ctrl+Alt+M" } } });
const hk6 = await invoke("hotkeys_status", {});
await invoke("settings_patch", { patch: { hotkeys: { mute: null } } });
const hk5 = await invoke("hotkeys_status", {});
check(
  "hotkey change applies without a restart",
  hk6.length === 6 && hk5.length === 5,
  `${hk6.length} → ${hk5.length}`,
);

// T1.23 «при закрытом окне»: ✕ hides the window, media controls still drive the player.
if (existsSync(join(libDir, ".sirtone", "library.db"))) {
  // The longest track, so it is still playing while the window is closed.
  const page1 = await invoke("collection_tracks", { after: null, limit: 200 });
  const longest = page1.rows.reduce((a, b) => (b.durationMs > a.durationMs ? b : a));
  await invoke("play_tracks", { ids: [longest.id], start: 0, context: "collection" });
  await sleep(1500);
  const playingNow = (await invoke("playback_snapshot", {})).title;
  spawnSync("powershell", [
    "-NoProfile",
    "-Command",
    `Add-Type -Name U -Namespace W -MemberDefinition '[DllImport("user32.dll")] public static extern bool PostMessageW(System.IntPtr h, uint m, System.IntPtr w, System.IntPtr l);'; [W.U]::PostMessageW((Get-Process -Id ${app.pid}).MainWindowHandle, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)`,
  ]);
  await sleep(800);
  const afterClose = spawnSync("powershell", [
    "-NoProfile",
    "-Command",
    `$t = (Get-Process -Id ${app.pid}).MainWindowTitle; if ($t -and $t -notlike '*-siw') { 'shown' } else { 'hidden' }`,
  ])
    .stdout.toString()
    .trim();
  const seen = spawnSync(cli, ["smtc"], { encoding: "utf8" }).stdout.trim();
  const tog = spawnSync(cli, ["smtc", "toggle", playingNow], { encoding: "utf8" });
  await sleep(700);
  const st2 = await invoke("playback_snapshot", {});
  check(
    "✕ hides; media controls still pause",
    afterClose === "hidden" && st2.state === "paused",
    `${afterClose}, ${st2.state}; want «${playingNow}»; toggle exit ${tog.status} ${tog.stderr}; smtc: ${seen}`,
  );
}

// 10. Autostart launches with --minimized (T1.24): straight to the tray, no window shown.
await browser.close();
app.kill();
await sleep(800);
app = launch(["--minimized"]);
({ browser, page } = await connect());
await sleep(4000); // past the show fallback
// The only visible top-level window may be a helper (single-instance «-siw»); the main window,
// titled like the app, must not be shown.
const shown = spawnSync("powershell", [
  "-NoProfile",
  "-Command",
  `$t = (Get-Process -Id ${app.pid}).MainWindowTitle; if ($t -and $t -notlike '*-siw') { 'shown' } else { 'hidden' }`,
])
  .stdout.toString()
  .trim();
check("--minimized keeps the window hidden", shown === "hidden", shown);
check("--minimized instance is alive", (await invoke("playback_snapshot", {})) !== null);

await browser.close();
app.kill();
await sleep(500);
rmSync(dataDir, { recursive: true, force: true });
const failed = results.filter((r) => !r.ok).length;
console.log(
  `\n${results.length - failed}/${results.length} passed (isolated data dir, owner's app untouched)`,
);
process.exit(failed ? 1 : 0);
