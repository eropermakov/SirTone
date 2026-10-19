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
  writeFileSync,
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
// Recorded MusicBrainz answers (AT-04): the isolated instance never calls the real service.
const netFixtures = join(dataDir, "net-fixtures");
mkdirSync(netFixtures, { recursive: true });
writeFileSync(join(netFixtures, "index.json"), "[]");
const env = {
  ...process.env,
  SIRTONE_DATA_DIR: dataDir,
  SIRTONE_NET_FIXTURES: netFixtures,
  // Analysis runs on battery too (an unplugged laptop must not fail the analysis checks).
  SIRTONE_SMOKE_AC: "1",
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
  // T3.21: «Подробнее» — the report in four sections.
  await report.getByRole("button", { name: "Подробнее" }).click();
  const full = page.getByRole("dialog", { name: "Итоги импорта" });
  await full.getByText(/Добавлено — 4\d\d/).waitFor({ timeout: 5000 });
  const fullText = (await full.textContent()) ?? "";
  check(
    "import report in full: added · «Неизвестные» · already there · not added",
    /Добавлено — \d+/.test(fullText) &&
      /Уже есть в медиатеке — 10/.test(fullText) &&
      /Не добавлено — 52/.test(fullText),
    fullText.slice(0, 160),
  );
  await page.keyboard.press("Escape");
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
  // The assigned track was queued right after the playing one: its queue item follows the
  // file, so skipping to it plays it from the new place.
  const queuedMoved = (await invoke("queue_get", {})).items[1];
  const movedRel = (() => {
    const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    try {
      return db.prepare("SELECT rel_path FROM track WHERE id = ?").get(queuedMoved?.trackId)
        ?.rel_path;
    } finally {
      db.close();
    }
  })();
  await invoke("playback_next", {});
  await sleep(1500);
  const skipped = await invoke("playback_snapshot", {});
  check(
    "a queued track moved by «Назначить» plays from its new path",
    skipped.state === "playing" &&
      skipped.trackId === queuedMoved?.trackId &&
      skipped.positionMs > 0 &&
      queuedMoved?.title === "Проверочная песня" &&
      /^Проверочный исполнитель\//.test(movedRel ?? ""),
    `${skipped.state} #${skipped.trackId} at ${skipped.positionMs} ms, queued «${queuedMoved?.title}» #${queuedMoved?.trackId}, ${movedRel}`,
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
  // T3.13: smart playlist from the sidebar — rules in the editor («Название содержит …»), the
  // live count, the page lists the matches; read-only; «Сохранить как обычный»; a hostile value
  // is plain text.
  const smartWord = (plListed ?? "").split(/\s+/).find((w) => w.length >= 3) ?? plListed ?? "";
  await page.getByRole("button", { name: "Новый умный плейлист" }).click();
  const smartDialog = page.getByRole("dialog", { name: "Правила умного плейлиста" });
  await smartDialog.waitFor({ timeout: 5000 });
  await smartDialog.getByLabel("Название плейлиста").fill("Умный тест");
  await smartDialog.getByLabel("Значение").fill(smartWord);
  await smartDialog.getByText(/Подходит \d+/).waitFor({ timeout: 5000 });
  const smartCount = (await smartDialog.getByRole("status").textContent())?.trim();
  // For a look at the editor (not checked): %TEMP%/sirtone-smart-editor.png.
  await smartDialog.screenshot({ path: join(tmpdir(), "sirtone-smart-editor.png") });
  await smartDialog.getByRole("button", { name: "Сохранить" }).click();
  const smartRows = page.getByRole("list", { name: "Треки" }).locator("li");
  await smartRows.first().waitFor({ timeout: 5000 });
  const smartList = (await invoke("playlists_list", {})).find((p) => p.name === "Умный тест");
  const smartTracks = await invoke("playlist_tracks", { id: smartList?.id ?? 0 });
  const allMatch = smartTracks.every((e) =>
    e.track.title
      .toLowerCase()
      .replaceAll("ё", "е")
      .includes(smartWord.toLowerCase().replaceAll("ё", "е")),
  );
  check(
    "smart playlist: built in the editor, live count, the page lists the matches",
    smartList?.smart === true &&
      smartTracks.length > 0 &&
      allMatch &&
      (await smartRows.count()) === smartTracks.length,
    `«${smartWord}»: ${smartCount}, ${smartTracks.length} tracks`,
  );
  const refused = await page.evaluate(
    (id) =>
      window.__TAURI_INTERNALS__.invoke("playlist_add", { id, tracks: [1] }).then(
        () => "accepted",
        (e) => String(e?.code ?? JSON.stringify(e)),
      ),
    smartList?.id ?? 0,
  );
  const tracksNow = (await invoke("library_status", {})).tracks;
  const hostile = await invoke("smart_preview", {
    rules: JSON.stringify({ all: [{ is: { title: "'); DROP TABLE track;--" } }] }),
  });
  await page.getByRole("main").getByRole("button", { name: "Ещё" }).first().click();
  await page.getByRole("menuitem", { name: "Сохранить как обычный" }).click();
  await sleep(1200);
  const copy = (await invoke("playlists_list", {})).find((p) => p.name === "Умный тест — копия");
  check(
    "smart playlist: read-only, «Сохранить как обычный», hostile value is text",
    refused === "InvalidArgument" &&
      copy?.smart === false &&
      copy?.tracks === smartTracks.length &&
      hostile === 0 &&
      (await invoke("library_status", {})).tracks === tracksNow,
    `add ${refused}, copy ${JSON.stringify(copy && { smart: copy.smart, tracks: copy.tracks })}, hostile ${hostile}`,
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
  // T4.1/T4.2: «Волны» behind the player — frames arrive at 60 Hz with small gaps, the canvas
  // is drawn (not blank), FPS measured; leaving and coming back keeps the feed alive (viz_on
  // counter); the palette comes from the cover and the collection.
  await page.locator('canvas[data-testid="waves"]').waitFor({ timeout: 5000 });
  await sleep(1500);
  const vizRead = () =>
    page.evaluate(() => ({
      frames: window.__sirtoneVizStats?.frames ?? 0,
      maxGap: window.__sirtoneVizStats?.maxGapMs ?? 0,
      fps: window.__sirtoneWaves?.stats.fps ?? 0,
      drawn: window.__sirtoneWaves?.stats.drawn ?? 0,
    }));
  await page.evaluate(() => {
    if (window.__sirtoneVizStats) window.__sirtoneVizStats.maxGapMs = 0;
  });
  const v0 = await vizRead();
  await sleep(3000);
  const v1 = await vizRead();
  const rate = (v1.frames - v0.frames) / 3;
  check(
    "viz: frames at ~60 Hz, max gap < 60 ms, waves drawn ≥ 30 FPS (P-12)",
    rate > 50 && v1.maxGap < 60 && v1.fps >= 30 && v1.drawn > v0.drawn,
    `${rate.toFixed(1)} frames/s, max gap ${v1.maxGap.toFixed(1)} ms, ${v1.fps.toFixed(1)} FPS`,
  );
  await page.evaluate(() => window.__sirtoneWaves?.sample());
  await sleep(300);
  const samples = await page.evaluate(() => window.__sirtoneWaves?.stats.samples ?? []);
  check(
    "viz: the waves canvas is painted (not blank, not flat)",
    samples.length === 5 &&
      samples.some((p) => p[0] + p[1] + p[2] > 30) &&
      new Set(samples.map((p) => p.join())).size > 1,
    JSON.stringify(samples),
  );
  const vizSnap = await invoke("playback_snapshot", {});
  const vizCols = await invoke("viz_colors", { trackId: vizSnap.trackId, cover: vizSnap.cover });
  check(
    "viz: palette from the cover and the collection",
    (!vizSnap.cover || vizCols.cover.length > 0) && vizCols.collection.length > 0,
    `cover ${vizCols.cover.length}, collection ${vizCols.collection.length}`,
  );
  if (process.env.SMOKE_SHOTS) {
    await page.screenshot({ path: join(process.env.SMOKE_SHOTS, "t42-waves.png") });
  }
  await page.getByRole("navigation").getByRole("button", { name: "Жанры" }).click();
  await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
  await page.locator('canvas[data-testid="waves"]').waitFor({ timeout: 5000 });
  await sleep(1000);
  const back0 = await vizRead();
  await sleep(1000);
  const back1 = await vizRead();
  check(
    "viz: back on the wave screen the feed runs again (no stale viz_on)",
    back1.frames - back0.frames > 40,
    `${back1.frames - back0.frames} frames/s`,
  );
  await axeScreen("«Моя волна»");
  // T2.20 / P-01 (DESIGN.md §1, `08` §1): event → the frame that shows the change, measured
  // in the page. Direct actions ≤ 100 ms, a screen change ≤ 400 ms.
  const reaction = (pick, changed) =>
    page.evaluate(
      async ([pickSrc, changedSrc]) => {
        const el = new Function(`return (${pickSrc})()`)();
        const done = new Function("el", "before", `return (${changedSrc})(el, before)`);
        const before = el.getAttribute("aria-pressed") ?? el.getAttribute("aria-label") ?? "";
        const t0 = performance.now();
        el.click();
        await new Promise((res) => {
          const tick = () =>
            done(el, before) ? requestAnimationFrame(() => res()) : requestAnimationFrame(tick);
          tick();
        });
        return performance.now() - t0;
      },
      [pick.toString(), changed.toString()],
    );
  const likeMs = [];
  for (let i = 0; i < 4; i++) {
    likeMs.push(
      await reaction(
        () => document.querySelector('main button[aria-label="Нравится"]'),
        (el, before) => el.getAttribute("aria-pressed") !== before,
      ),
    );
    await sleep(300);
  }
  const playMs = [];
  for (let i = 0; i < 4; i++) {
    playMs.push(
      await reaction(
        () => document.querySelectorAll("footer button[aria-label]")[2],
        (el, before) => el.getAttribute("aria-label") !== before,
      ),
    );
    await sleep(400);
  }
  const navMs = await reaction(
    () =>
      [...document.querySelectorAll("nav button")].find((b) => b.textContent?.trim() === "Альбомы"),
    () => document.querySelector("h1")?.textContent === "Альбомы",
  );
  check(
    "P-01: ♡ and play/pause ≤ 100 ms, a screen change ≤ 400 ms",
    Math.max(...likeMs) <= 100 && Math.max(...playMs) <= 100 && navMs <= 400,
    `♡ ${likeMs.map((v) => v.toFixed(0)).join("/")} ms, play ${playMs.map((v) => v.toFixed(0)).join("/")} ms, screen ${navMs.toFixed(0)} ms`,
  );
  await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
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
  // Stage 3: with an album playing, «Моя волна» offers «Слушать волну»; the engine queues 5,
  // refills as they run out; a not-yet-played track marked «никогда» never comes.
  await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
  const startBtn = page.getByRole("main").getByRole("button", { name: "Слушать волну" });
  const offered = await startBtn.waitFor({ timeout: 3000 }).then(
    () => true,
    () => false,
  );
  if (offered) await startBtn.click();
  await sleep(1500);
  const q0 = await invoke("queue_get", {});
  const inWave = (await invoke("playback_snapshot", {})).inWave;
  const unplayed = q0.items[q0.items.length - 1]?.trackId;
  await invoke("feedback_never", { scope: "track", id: unplayed, on: true });
  await invoke("wave_start", {});
  const fresh = await invoke("queue_get", {});
  for (let i = 0; i < 4; i++) {
    await invoke("playback_next", {});
    await sleep(700);
  }
  await sleep(1500);
  const q1 = await invoke("queue_get", {});
  const ids = [...fresh.items, ...q1.items].map((it) => it.trackId);
  check(
    "wave: «Слушать волну» from a playing album, 5 → refilled, «никогда» excluded",
    offered && inWave && q0.items.length === 5 && q1.items.length > 5 && !ids.includes(unplayed),
    `offered ${offered}, inWave ${inWave}, queue ${q0.items.length} → ${q1.items.length}`,
  );
  await invoke("feedback_never", { scope: "track", id: unplayed, on: false });
  // T3.29: «Почему этот трек» — 1–2 reasons for a track of the running wave; «Метки ›» saves.
  await invoke("wave_start", {});
  await sleep(800);
  const waveTrack = (await invoke("playback_snapshot", {})).trackId;
  const reasons = await invoke("wave_why", { track: waveTrack });
  await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
  await page.getByRole("main").getByRole("button", { name: "Ещё" }).click();
  await page.getByRole("menuitem", { name: "Метки" }).click();
  await page.getByRole("menuitem", { name: "Для сна" }).click();
  await sleep(600);
  const tags = await invoke("track_tags", { track: waveTrack });
  check(
    "why: 1–2 reasons; «Метки › Для сна» is saved",
    reasons.length >= 1 && reasons.length <= 2 && tags.includes("sleep"),
    `${reasons.map((r) => r.kind).join(", ")}; tags ${tags.join(",")}`,
  );
  // T3.31: «Знакомое ↔ Неожиданное» — the upcoming wave tracks are picked again at once.
  const qBefore = await invoke("queue_get", {});
  const upBefore = qBefore.items
    .slice(qBefore.position + 1)
    .map((i) => i.trackId)
    .join(",");
  const slider = page.getByRole("slider", { name: "Знакомое или неожиданное" });
  const box = await slider.boundingBox();
  await page.mouse.click(box.x + 2, box.y + box.height / 2);
  for (let i = 0; i < 15 && (settings().wave?.novelty ?? 50) > 10; i++) await sleep(200);
  await sleep(1500);
  const qAfter = await invoke("queue_get", {});
  const upAfter = qAfter.items
    .slice(qAfter.position + 1)
    .map((i) => i.trackId)
    .join(",");
  check(
    "slider: saved and the upcoming wave tracks re-picked",
    (settings().wave?.novelty ?? 50) <= 10 && upAfter !== upBefore && upAfter !== "",
    `novelty ${settings().wave?.novelty}`,
  );
  // T3.30: «Сезонная волна» — named by the calendar; started by its button while «Моя волна»
  // plays, and «Слушать волну» leads back.
  const seasonKey = await invoke("wave_season_card", {});
  const shellRu = JSON.parse(
    readFileSync(new URL("../../../assets/i18n/ru/shell.json", import.meta.url), "utf8"),
  );
  const seasonLabel = shellRu.wave.season[seasonKey];
  await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
  await page.getByRole("main").getByRole("button", { name: seasonLabel }).click();
  let seasonSnap = await invoke("playback_snapshot", {});
  for (let i = 0; i < 20 && seasonSnap.waveProfile !== "seasonal"; i++) {
    await sleep(200);
    seasonSnap = await invoke("playback_snapshot", {});
  }
  const sq = await invoke("queue_get", {});
  const back = await page
    .getByRole("main")
    .getByRole("button", { name: "Слушать волну" })
    .isVisible();
  check(
    "seasonal wave: named by the calendar, started by its button, «Моя волна» offered back",
    ["autumn", "earlyAutumn", "preWinter", "earlyWinter", "winter"].includes(seasonKey) &&
      seasonSnap.waveProfile === "seasonal" &&
      sq.items.length === 5 &&
      back,
    `${seasonKey} «${seasonLabel}», profile ${seasonSnap.waveProfile}, queue ${sq.items.length}, back ${back}`,
  );
  // T3.18: «Настроить волну» — a mood needs deep analysis (not yet): the wave relaxes it and
  // still plays; frequency «любимые» + Russian hold or relax in order; a bad key is refused.
  const moodStart = await invoke("wave_start", { query: { mood: ["happy"] } });
  // The rejection is caught in the page: the AppError object does not survive page.evaluate.
  const bad = await page.evaluate(() =>
    window.__TAURI_INTERNALS__.invoke("wave_start", { query: { mood: ["sunny"] } }).then(
      () => "accepted",
      (e) => String(e?.code ?? JSON.stringify(e)),
    ),
  );
  const freqStart = await invoke("wave_start", {
    query: { frequency: ["often"], language: ["ru", "foreign"] },
  });
  check(
    "wave query: unanalysed mood relaxed, the wave still plays; bad key refused",
    moodStart.queued === 5 &&
      moodStart.relaxed.includes("mood") &&
      freqStart.queued > 0 &&
      bad.includes("InvalidArgument"),
    `mood ${JSON.stringify(moodStart)}, often ${JSON.stringify(freqStart)}, bad ${bad}`,
  );
  // T3.28: learning off freezes the models (the wave still plays); «Сбросить обучение» works.
  await page.getByRole("navigation").getByRole("button", { name: "Настройки" }).click();
  await page.getByRole("switch", { name: "Учиться на прослушиваниях" }).click();
  for (let i = 0; i < 15 && settings().wave?.learning !== false; i++) await sleep(200);
  const frozenStart = await invoke("wave_start", {});
  await invoke("wave_reset_learning", {});
  await page.getByRole("switch", { name: "Учиться на прослушиваниях" }).click();
  for (let i = 0; i < 15 && settings().wave?.learning !== true; i++) await sleep(200);
  check(
    "learning off: the wave still plays; reset works; the switch is saved",
    frozenStart.queued === 5 && settings().wave?.learning === true,
    `start ${frozenStart.queued}, learning ${settings().wave?.learning}`,
  );
  // T2.14: «Хранилище» shows the service data next to the library.
  await page.getByRole("navigation").getByRole("button", { name: "Настройки" }).click();
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
  // T3.6/T3.10: background analysis B0 runs on its own (newest first) and stores loudness.
  let prog = await invoke("analysis_progress", {});
  for (let i = 0; i < 30 && prog.done === 0; i++) {
    await sleep(1000);
    prog = await invoke("analysis_progress", {});
  }
  // T3.2: the same pass stores fingerprints (Chromaprint, compressed).
  let fps = 0;
  for (let i = 0; i < 30 && fps === 0; i++) {
    const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    fps = db.prepare("SELECT count(*) AS n FROM track WHERE fingerprint IS NOT NULL").get().n;
    db.close();
    if (fps === 0) await sleep(1000);
  }
  check("fingerprints are stored by the analysis", fps > 0, `${fps} tracks`);
  check(
    "background analysis B0 is running",
    prog.done > 0 && prog.total > 0,
    `${prog.done} of ${prog.total}, paused ${prog.paused}`,
  );
  // T3.10 acceptance: analysis (B0 + fingerprint + B1) in the background causes no underrun and
  // no UI stall — 20 s of playback while tracks are still being analysed.
  await invoke("wave_start", {});
  await sleep(1000);
  const under0 = (await invoke("playback_snapshot", {})).underruns;
  const progA = await invoke("analysis_progress", {});
  let worstIpc = 0;
  for (let i = 0; i < 20; i++) {
    const t0 = performance.now();
    await invoke("library_status", {});
    worstIpc = Math.max(worstIpc, performance.now() - t0);
    await sleep(1000);
  }
  const snapA = await invoke("playback_snapshot", {});
  const progB = await invoke("analysis_progress", {});
  check(
    "analysis in the background: no underrun, IPC ≤ 100 ms while playing (T3.10)",
    snapA.state === "playing" &&
      snapA.underruns === under0 &&
      worstIpc <= 100 &&
      progB.done > progA.done,
    `underruns ${under0} → ${snapA.underruns}, worst IPC ${worstIpc.toFixed(1)} ms, analysed ${progA.done} → ${progB.done}`,
  );
  await invoke("playback_toggle", {});
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
  // T3.13: a live smart playlist «путь содержит Снаружи» follows the folder — rows and header.
  await invoke("smart_create", {
    name: "Снаружи",
    rules: JSON.stringify({ all: [{ contains: { path: "Снаружи" } }] }),
  });
  await page.getByRole("navigation").getByRole("button", { name: "Снаружи" }).click();
  const outsideRows = page.getByRole("list", { name: "Треки" }).locator("li");
  const before = await tracks();
  mkdirSync(join(libDir, "Снаружи"), { recursive: true });
  const added = join(libDir, "Снаружи", "Искала.flac");
  copyFileSync(join(root, "fixtures", "generated", "tags", "cyrillic-ok.flac"), added);
  const [n1, ms1] = await waitTracks(before + 1, 5000);
  check("file added in Explorer appears ≤ 2 s", n1 === before + 1 && ms1 <= 2000, `${ms1} ms`);
  // T3.19: it is taken where it is, and «Навести порядок» is offered (never moved silently).
  const organizeOffered = await page
    .getByText("Новые файлы в папке медиатеки")
    .waitFor({ timeout: 5000 })
    .then(
      () => true,
      () => false,
    );
  const keptInPlace = existsSync(added);
  const outsideHeader = page.getByRole("main").getByText(/^1 трек ·/);
  const live = await outsideHeader.waitFor({ timeout: 3000 }).then(
    () => true,
    () => false,
  );
  check(
    "live smart playlist follows the folder: the row and «1 трек» in the header",
    live && (await outsideRows.count()) === 1,
    `rows ${await outsideRows.count()}, header ${live}`,
  );
  // T3.5 / AT-05: the same melody as FLAC and MP3 under other names comes in through Explorer;
  // the analysis fingerprints it and finds «тот же звук»; the old one gets a ♡ and a playlist
  // place; «Заменить старый новым» in the «Дубли» dialog moves them and sends the old file to
  // the Recycle Bin.
  const dupDir = join(libDir, "Дубли");
  mkdirSync(dupDir, { recursive: true });
  const dupSrc = join(root, "fixtures", "generated", "dups");
  copyFileSync(join(dupSrc, "Утро.flac"), join(dupDir, "Утро.flac"));
  await sleep(1500);
  copyFileSync(join(dupSrc, "Рассвет.mp3"), join(dupDir, "Рассвет.mp3"));
  copyFileSync(join(dupSrc, "Вечер.flac"), join(dupDir, "Вечер.flac"));
  const dupIds = () => {
    const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    const rows = db
      .prepare("SELECT id, rel_path FROM track WHERE rel_path LIKE 'Дубли/%' ORDER BY id")
      .all();
    db.close();
    return Object.fromEntries(rows.map((r) => [r.rel_path.split("/")[1], r.id]));
  };
  let dupCount = 0;
  for (let i = 0; i < 90 && dupCount === 0; i++) {
    await sleep(1000);
    dupCount = await invoke("duplicates_count", {});
  }
  const dupMap = dupIds();
  // T3.7: the same analysis pass measured tempo and key (B1) of these 25 s melodies.
  const b1db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
  const b1 = b1db
    .prepare("SELECT bpm, key_camelot FROM track WHERE rel_path LIKE 'Дубли/%' AND bpm IS NOT NULL")
    .all();
  // T3.8/T3.9: the built-in tagger in the same pass — vibe columns, embedding, wave vector.
  const deep = b1db
    .prepare(
      `SELECT t.energy, t.vocal, length(t.embedding) AS emb, length(tf.vec) AS vec, tf.conf,
              t.activity_json AS acts
       FROM track t JOIN track_features tf ON tf.track_id = t.id
       WHERE t.rel_path LIKE 'Дубли/%'`,
    )
    .all();
  b1db.close();
  check(
    "B2/B3: offline tagger stored the vibe, the embedding and the wave vector",
    deep.length >= 2 &&
      deep.every(
        (r) =>
          r.energy >= 0 &&
          r.energy <= 1 &&
          r.vocal >= 0 &&
          r.emb === 68 &&
          r.vec === 128 &&
          r.conf > 0 &&
          JSON.parse(r.acts).party !== undefined,
      ),
    JSON.stringify(deep),
  );
  check(
    "B1: tempo (70–180) and Camelot key stored by the analysis",
    b1.length >= 2 &&
      b1.every((r) => r.bpm >= 70 && r.bpm <= 180 && /^\d{1,2}[AB]$/.test(r.key_camelot)),
    JSON.stringify(b1),
  );
  const roadPl = (await invoke("playlists_list", {})).find((p) => p.name === "Дорога");
  const oldId = dupMap["Утро.flac"];
  const newId = dupMap["Рассвет.mp3"];
  await invoke("feedback_like", { track: oldId, on: true });
  await invoke("playlist_add", { id: roadPl.id, tracks: [oldId] });
  await page.getByRole("navigation").getByRole("button", { name: "Настройки" }).click();
  await page
    .getByText(/^Дубли: /)
    .locator("..")
    .getByRole("button", { name: "Показать" })
    .click();
  const dupDialog = page.getByRole("dialog", { name: "Дубли" });
  await dupDialog.waitFor({ timeout: 5000 });
  // The list loads after the dialog opens.
  const sameHeader = await dupDialog
    .getByText(/Тот же звук/)
    .waitFor({ timeout: 5000 })
    .then(
      () => 1,
      () => 0,
    );
  const dupListed = await invoke("duplicates_list", {});
  await dupDialog
    .getByRole("listitem")
    .filter({ hasText: "Рассвет (копия)" })
    .getByRole("button", { name: "Заменить старый новым" })
    .click();
  await sleep(2500);
  const dupDb = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
  const liked = dupDb
    .prepare("SELECT count(*) AS n FROM feedback WHERE track_id = ? AND kind = 'like'")
    .get(newId).n;
  const inRoad = dupDb
    .prepare("SELECT count(*) AS n FROM playlist_item WHERE playlist_id = ? AND track_id = ?")
    .get(roadPl.id, newId).n;
  const oldRow = dupDb.prepare("SELECT count(*) AS n FROM track WHERE id = ?").get(oldId).n;
  const eveningPairs = dupDb
    .prepare("SELECT count(*) AS n FROM duplicate_pair WHERE a = ? OR b = ?")
    .get(dupMap["Вечер.flac"], dupMap["Вечер.flac"]).n;
  dupDb.close();
  await page.keyboard.press("Escape");
  check(
    "AT-05: «тот же звук» under other names found; «Заменить» moves ♡ and playlist, old file → Recycle Bin",
    dupCount >= 1 &&
      sameHeader === 1 &&
      liked === 1 &&
      inRoad === 1 &&
      oldRow === 0 &&
      !existsSync(join(dupDir, "Утро.flac")) &&
      eveningPairs === 0,
    `header ${sameHeader}, pairs ${dupCount} (${dupListed.pairs.map((p) => `${p.class} ${p.a.track.title}↔${p.b.track.title}`).join("; ")}), ♡ ${liked}, playlist ${inRoad}, old row ${oldRow}, file ${existsSync(join(dupDir, "Утро.flac"))}, «Вечер» pairs ${eveningPairs}`,
  );
  // The rest of the fixtures leave (the later checks count tracks).
  rmSync(dupDir, { recursive: true, force: true });
  await waitTracks(before + 1, 8000);
  // T3.12 / AT-06: one artist in two scripts comes in through Explorer → the suggestion banner
  // → «Объединить» moves the files under the kept artist → «Отменить» brings them back → after
  // merging again, a new file under the old spelling lands with the kept artist (the alias).
  const mergeDir = join(libDir, "Слияние");
  mkdirSync(mergeDir, { recursive: true });
  const mergeSrc = join(root, "fixtures", "generated", "merge");
  copyFileSync(join(mergeSrc, "bi2-cyr.mp3"), join(mergeDir, "bi2-cyr.mp3"));
  copyFileSync(join(mergeSrc, "bi2-lat.mp3"), join(mergeDir, "bi2-lat.mp3"));
  await waitTracks(before + 3, 8000);
  const mergeRows = (sql, ...args) => {
    const mdb = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    const rows = mdb.prepare(sql).all(...args);
    mdb.close();
    return rows;
  };
  const pathOf = (title) =>
    mergeRows("SELECT rel_path FROM track WHERE title = ?", title)[0]?.rel_path ?? "";
  const mergedInto = (id) =>
    mergeRows("SELECT merged_into FROM artist WHERE id = ?", id)[0]?.merged_into ?? null;
  const pair = (await invoke("artist_suggestions", {})).find((s) =>
    [s.keep.name, s.drop.name].includes("Bi-2"),
  );
  await page.getByRole("navigation").getByRole("button", { name: "Исполнители" }).click();
  const banner = page.getByRole("status").filter({ hasText: "Похоже, это один исполнитель" });
  const bannerShown = await banner.waitFor({ timeout: 8000 }).then(
    () => true,
    () => false,
  );
  if (bannerShown) await banner.getByRole("button", { name: "Объединить" }).click();
  for (let i = 0; i < 50 && pair && mergedInto(pair.drop.id) === null; i++) await sleep(200);
  const merged = pair ? mergedInto(pair.drop.id) === pair.keep.id : false;
  const dropTitle = pair?.drop.name === "Bi-2" ? "Molitva" : "Серебро";
  // The files move after the database merge, track by track.
  let movedTo = pathOf(dropTitle);
  for (let i = 0; i < 50 && pair && !movedTo.startsWith(`${pair.keep.name}/`); i++) {
    await sleep(200);
    movedTo = pathOf(dropTitle);
  }
  await page.getByRole("button", { name: "Отменить" }).click();
  for (let i = 0; i < 50 && pair && mergedInto(pair.drop.id) !== null; i++) await sleep(200);
  const undone = pair ? mergedInto(pair.drop.id) === null : false;
  let backTo = pathOf(dropTitle);
  for (let i = 0; i < 50 && backTo === movedTo; i++) {
    await sleep(200);
    backTo = pathOf(dropTitle);
  }
  // Merge again, then a file under the dropped spelling arrives.
  if (pair) await invoke("artists_merge", { keep: pair.keep.id, drop: pair.drop.id });
  copyFileSync(join(mergeSrc, "bi2-lat2.mp3"), join(mergeDir, "bi2-lat2.mp3"));
  await waitTracks(before + 4, 8000);
  const lateArtist = mergeRows(
    `SELECT ta.artist_id AS a FROM track t JOIN track_artist ta ON ta.track_id = t.id
     WHERE t.title = 'Varvara' AND ta.role = 'primary'`,
  )[0]?.a;
  check(
    "AT-06: two spellings suggested, merged by the banner (files move), undone, alias remembered",
    !!pair &&
      bannerShown &&
      merged &&
      movedTo.startsWith(`${pair.keep.name}/`) &&
      undone &&
      backTo !== movedTo &&
      lateArtist === pair.keep.id,
    `pair ${pair?.keep.name}←${pair?.drop.name}, banner ${bannerShown}, merged ${merged} → ${movedTo}, undone ${undone} → ${backTo}, late → ${lateArtist}`,
  );
  // The fixtures leave (later checks count tracks).
  for (const title of ["Серебро", "Molitva", "Varvara"]) {
    const rel = pathOf(title);
    if (rel) rmSync(join(libDir, ...rel.split("/")), { force: true });
  }
  rmSync(mergeDir, { recursive: true, force: true });
  await waitTracks(before + 1, 8000);
  // AT-07 (T3.14): a playlist of three analysed tracks → similar ones under the divider, «−»
  // never comes back, «+» moves a track into the playlist, ∞ continues the queue with
  // recommendations (journal source «hybrid»), ∞ off ends it. The stop when nothing is close
  // enough is proven on constructed vectors (`sirtone-wave` similar::tests).
  const analysed = async () => {
    const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    try {
      return db
        .prepare("SELECT id FROM track WHERE state = 'ok' AND embedding IS NOT NULL ORDER BY id")
        .all()
        .map((r) => r.id);
    } finally {
      db.close();
    }
  };
  let withEmb = await analysed();
  for (let i = 0; i < 120 && withEmb.length < 8; i++) {
    await sleep(1000);
    withEmb = await analysed();
  }
  const at07 = await invoke("playlist_create", { name: "AT-07" });
  const seeds07 = withEmb.slice(0, 3);
  await invoke("playlist_add", { id: at07, tracks: seeds07 });
  const pre1 = await invoke("continuation_preview", { playlist: at07 });
  const recIds = pre1.map((x) => x.id);
  const minusId = recIds[0];
  const plusId = recIds[1];
  if (minusId !== undefined) {
    await invoke("continuation_exclude", { playlist: at07, track: minusId });
  }
  if (plusId !== undefined) await invoke("playlist_add", { id: at07, tracks: [plusId] });
  const pre2 = (await invoke("continuation_preview", { playlist: at07 })).map((x) => x.id);
  const inList = (await invoke("playlist_tracks", { id: at07 })).map((e) => e.track.id);
  check(
    "AT-07: similar tracks under the divider; «−» never returns; «+» moves into the playlist",
    recIds.length >= 2 &&
      recIds.every((x) => !seeds07.includes(x)) &&
      !pre2.includes(minusId) &&
      !pre2.includes(plusId) &&
      inList.includes(plusId),
    `analysed ${withEmb.length}, preview ${JSON.stringify(recIds)} → ${JSON.stringify(pre2)}, playlist ${JSON.stringify(inList)}`,
  );
  // The page: the divider with ∞ and the rows with «+»/«−» in the same scroll.
  await page.getByRole("navigation").getByRole("button", { name: "AT-07" }).click();
  const recSection = page.getByRole("region", { name: "Рекомендовано из вашей медиатеки" });
  await recSection.waitFor({ timeout: 5000 }).catch(() => {});
  const recRows = recSection.locator("[data-recommended]");
  for (let i = 0; i < 20 && (await recRows.count()) === 0; i++) await sleep(250);
  const rowsBefore = await recRows.count();
  const infPressed = await recSection
    .getByRole("button", { name: "Продолжать похожими (∞)" })
    .getAttribute("aria-pressed");
  if (process.env.SMOKE_SHOTS) {
    await page.screenshot({ path: join(process.env.SMOKE_SHOTS, "at07-playlist.png") });
  }
  const firstRec = await recRows.first().getAttribute("data-recommended");
  await recRows.first().hover();
  await recRows.first().getByRole("button", { name: "Не предлагать" }).click();
  let rowsAfter = await recRows.count();
  for (let i = 0; i < 10 && rowsAfter === rowsBefore; i++) {
    await sleep(200);
    rowsAfter = await recRows.count();
  }
  const preUi = (await invoke("continuation_preview", { playlist: at07 })).map((x) => x.id);
  check(
    "AT-07 UI: «Рекомендовано из вашей медиатеки» with ∞ on; «−» on a row removes it for good",
    rowsBefore >= 2 &&
      infPressed === "true" &&
      rowsAfter === rowsBefore - 1 &&
      !preUi.includes(Number(firstRec)),
    `rows ${rowsBefore} → ${rowsAfter}, ∞ ${infPressed}, removed ${firstRec}`,
  );
  // Play the playlist from its last track: ∞ appends recommendations behind it.
  await invoke("play_tracks", {
    ids: inList,
    start: inList.length - 1,
    context: "playlist",
    playlist: at07,
  });
  let q07 = await invoke("queue_get", {});
  for (let i = 0; i < 40 && !q07.items.some((x) => x.recommended); i++) {
    await sleep(250);
    q07 = await invoke("queue_get", {});
  }
  const recQueued = q07.items.filter((x) => x.recommended).map((x) => x.trackId);
  check(
    "AT-07: ∞ continues the playlist with recommendations (not «−», not the playlist's own)",
    q07.autoplay === true &&
      q07.playlist === at07 &&
      recQueued.length >= 1 &&
      q07.items.findIndex((x) => x.recommended) === inList.length &&
      recQueued.every((x) => x !== minusId && !inList.includes(x)),
    `autoplay ${q07.autoplay}, queue ${JSON.stringify(q07.items.map((x) => [x.trackId, x.recommended]))}`,
  );
  // Into the first recommendation and on: its play is journalled as «hybrid».
  await invoke("playback_next", {});
  await sleep(1500);
  await invoke("playback_next", {});
  let hybridPlays = [];
  for (let i = 0; i < 20 && hybridPlays.length === 0; i++) {
    await sleep(250);
    hybridPlays = readJournal().filter((r) => r.source === "hybrid");
  }
  // ∞ off: the queued recommendations go, the queue ends after the playlist.
  await invoke("continuation_set", { playlist: at07, on: false });
  const qOff = await invoke("queue_get", {});
  const pos = qOff.position;
  check(
    "AT-07: a recommendation plays as «hybrid»; ∞ off drops the upcoming ones",
    hybridPlays.length >= 1 &&
      qOff.autoplay === false &&
      !qOff.items.slice(pos + 1).some((x) => x.recommended),
    `hybrid plays ${hybridPlays.length}, after off ${JSON.stringify(qOff.items.slice(pos).map((x) => x.recommended))}`,
  );
  const snap07 = await invoke("playback_snapshot", {});
  if (snap07.state === "playing") await invoke("playback_toggle", {});
  // T3.1: «Работать полностью офлайн» — the switch reaches the network layer at once.
  await page.getByRole("navigation").getByRole("button", { name: "Настройки" }).click();
  await page.getByRole("switch", { name: "Работать полностью офлайн" }).click();
  let netOn = await invoke("net_status", {});
  for (let i = 0; i < 15 && !netOn.forced; i++) {
    await sleep(200);
    netOn = await invoke("net_status", {});
  }
  await page.getByRole("switch", { name: "Работать полностью офлайн" }).click();
  let netOff = await invoke("net_status", {});
  for (let i = 0; i < 15 && netOff.forced; i++) {
    await sleep(200);
    netOff = await invoke("net_status", {});
  }
  check(
    "offline switch: every request blocked while on, back when off",
    netOn.forced && netOn.offline && !netOff.forced && settings().network?.offline_mode === false,
    `on ${JSON.stringify(netOn)}, off ${JSON.stringify(netOff)}`,
  );
  // T3.19: «Навести порядок во всей медиатеке» puts the Explorer file where the rules say.
  await page.getByRole("navigation").getByRole("button", { name: "Настройки" }).click();
  await page.getByRole("button", { name: "Навести порядок во всей медиатеке" }).click();
  const relOf = (title) => {
    const odb = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    const r = odb.prepare("SELECT rel_path FROM track WHERE title = ?").get(title);
    odb.close();
    return r?.rel_path ?? "";
  };
  let organized = relOf("Искала");
  for (let i = 0; i < 50 && organized.startsWith("Снаружи/"); i++) {
    await sleep(200);
    organized = relOf("Искала");
  }
  check(
    "T3.19: Explorer file kept in place, «Навести порядок» offered, then laid out by the rules",
    organizeOffered && keptInPlace && organized !== "" && !organized.startsWith("Снаружи/"),
    `offered ${organizeOffered}, kept ${keptInPlace}, now ${organized}`,
  );
  rmSync(join(libDir, ...organized.split("/")), { force: true });
  rmSync(added, { force: true });
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
  // AT-04 (FR-REC-006, D-87): offline, an untagged «Артист - Название» file waits in
  // «Неизвестные» and import is not blocked; the network comes back → MusicBrainz (recorded
  // answers) confirms artist, title and length → the file moves to its place.
  await invoke("settings_patch", { patch: { network: { offline_mode: true } } });
  const at04Dir = join(dataDir, "at04-src");
  mkdirSync(at04Dir, { recursive: true });
  {
    // 5 s of a 523 Hz tone, 22.05 kHz mono 16-bit PCM, no tags.
    const rate = 22050;
    const n = rate * 5;
    const buf = Buffer.alloc(44 + n * 2);
    buf.write("RIFF", 0);
    buf.writeUInt32LE(36 + n * 2, 4);
    buf.write("WAVEfmt ", 8);
    buf.writeUInt32LE(16, 16);
    buf.writeUInt16LE(1, 20);
    buf.writeUInt16LE(1, 22);
    buf.writeUInt32LE(rate, 24);
    buf.writeUInt32LE(rate * 2, 28);
    buf.writeUInt16LE(2, 32);
    buf.writeUInt16LE(16, 34);
    buf.write("data", 36);
    buf.writeUInt32LE(n * 2, 40);
    for (let i = 0; i < n; i++) {
      buf.writeInt16LE(Math.round(8000 * Math.sin((2 * Math.PI * 523 * i) / rate)), 44 + i * 2);
    }
    writeFileSync(join(at04Dir, "Сплин - Романс.wav"), buf);
  }
  await invoke("import_paths", { paths: [at04Dir] });
  const at04Row = () => {
    const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    try {
      return db
        .prepare(
          "SELECT id, rel_path, identity_state, unknown_reason, mbid_recording, duration_ms FROM track WHERE rel_path LIKE '%Романс%'",
        )
        .get();
    } finally {
      db.close();
    }
  };
  let r04 = at04Row();
  for (let i = 0; i < 40 && !r04; i++) {
    await sleep(250);
    r04 = at04Row();
  }
  const offlineState = r04 ? `${r04.identity_state}/${r04.unknown_reason} ${r04.rel_path}` : "none";
  // The recorded answer: «Сплин — Романс», the file's own length, an official album of 1998.
  const rg04 = "a1b2c3d4-0000-4000-8000-000000000098";
  writeFileSync(
    join(netFixtures, "at04-search.json"),
    JSON.stringify({
      count: 1,
      recordings: [
        {
          id: "f00dbabe-0000-4000-8000-000000000004",
          score: "100",
          title: "Романс",
          length: r04?.duration_ms ?? 5000,
          "artist-credit": [{ name: "Сплин" }],
          releases: [
            {
              title: "Гранатовый альбом",
              status: "Official",
              date: "1998-03-01",
              "release-group": { id: rg04, "primary-type": "Album" },
              media: [{ track: [{ number: "7" }] }],
            },
          ],
        },
      ],
    }),
  );
  // T3.3: the Cover Art Archive answers with a front cover for the release group — a picture
  // of its own (a library cover would be stored once, under its first source).
  {
    const { deflateSync, crc32 } = require("node:zlib");
    const chunk = (type, data) => {
      const len = Buffer.alloc(4);
      len.writeUInt32BE(data.length);
      const td = Buffer.concat([Buffer.from(type), data]);
      const crc = Buffer.alloc(4);
      crc.writeUInt32BE(crc32(td) >>> 0);
      return Buffer.concat([len, td, crc]);
    };
    const w = 64;
    const ihdr = Buffer.alloc(13);
    ihdr.writeUInt32BE(w, 0);
    ihdr.writeUInt32BE(w, 4);
    ihdr[8] = 8;
    ihdr[9] = 2;
    const raw = Buffer.alloc((w * 3 + 1) * w);
    for (let y = 0; y < w; y++) {
      raw[y * (w * 3 + 1)] = 0;
      for (let x = 0; x < w; x++) {
        const o = y * (w * 3 + 1) + 1 + x * 3;
        raw[o] = (x * 4) & 255;
        raw[o + 1] = (y * 4) & 255;
        raw[o + 2] = 137;
      }
    }
    writeFileSync(
      join(netFixtures, "at04-cover.jpg"),
      Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        chunk("IHDR", ihdr),
        chunk("IDAT", deflateSync(raw)),
        chunk("IEND", Buffer.alloc(0)),
      ]),
    );
  }
  writeFileSync(
    join(netFixtures, "at04-rg.json"),
    JSON.stringify({ id: rg04, "first-release-date": "1998-03-01" }),
  );
  writeFileSync(
    join(netFixtures, "index.json"),
    JSON.stringify([
      // «романс», form-encoded.
      { contains: "%D1%80%D0%BE%D0%BC%D0%B0%D0%BD%D1%81", file: "at04-search.json" },
      { contains: `release-group/${rg04}/front`, file: "at04-cover.jpg" },
      { contains: `release-group/${rg04}`, file: "at04-rg.json" },
    ]),
  );
  await invoke("settings_patch", { patch: { network: { offline_mode: false } } });
  let r04b = at04Row();
  for (let i = 0; i < 60 && r04b?.identity_state !== "confirmed"; i++) {
    await sleep(250);
    r04b = at04Row();
  }
  check(
    "AT-04: offline → «Неизвестные», import not blocked; network back → confirmed online and moved into place",
    r04?.identity_state === "unknown" &&
      r04b?.identity_state === "confirmed" &&
      r04b.mbid_recording === "f00dbabe-0000-4000-8000-000000000004" &&
      r04b.rel_path.startsWith("Сплин/") &&
      r04b.rel_path.includes("Гранатовый альбом") &&
      existsSync(join(libDir, ...r04b.rel_path.split("/"))),
    `offline ${offlineState} → ${r04b ? `${r04b.identity_state} ${r04b.rel_path}` : "none"}`,
  );
  // T3.3 (FR-REC-009): the album placed online gets its cover from the Cover Art Archive.
  const coverOf = () => {
    const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    try {
      return db
        .prepare(
          "SELECT al.mbid_group, cv.source FROM track t JOIN album al ON al.id = t.album_id LEFT JOIN cover cv ON cv.id = al.cover_id WHERE t.rel_path LIKE '%Романс%'",
        )
        .get();
    } finally {
      db.close();
    }
  };
  let cov = coverOf();
  for (let i = 0; i < 40 && cov?.source !== "caa"; i++) {
    await sleep(250);
    cov = coverOf();
  }
  check(
    "T3.3: the album confirmed online gets its cover from the Cover Art Archive",
    cov?.mbid_group === rg04 && cov?.source === "caa",
    JSON.stringify(cov),
  );
  rmSync(at04Dir, { recursive: true, force: true });

  // T3.16 (FR-SRCH-004/005): a smart query is understood offline — «По смыслу» lists tracks
  // that really are sad and slow, «Играть этот вайб» starts the wave with those filters;
  // nothing found → the library's closest artists.
  const vq = "что-то грустное и медленное";
  const vr = await invoke("library_search", { query: vq });
  const vibeIds = (vr.vibe?.tracks ?? []).map((x) => x.id);
  let vibeOk = true;
  if (vibeIds.length > 0) {
    const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    try {
      const st = db.prepare("SELECT mood_bits, bpm FROM track WHERE id = ?");
      vibeOk = vibeIds.every((id) => {
        const r = st.get(id);
        return (r.mood_bits & 8) !== 0 && r.bpm >= 0 && r.bpm <= 90;
      });
    } finally {
      db.close();
    }
  }
  // Too far for the typo correction, close enough to suggest.
  const sugg = await invoke("library_search", { query: "земфираааааа" });
  const foundZ =
    sugg.suggestions.some((a) => a.name === "Земфира") ||
    sugg.artists.some((a) => a.name === "Земфира");
  await page.keyboard.press("Control+f");
  const sf = page.getByLabel("Поиск по медиатеке");
  await sf.waitFor({ timeout: 3000 });
  await sf.fill(vq);
  const vibeBtn = page.getByRole("button", { name: "Играть этот вайб" }).first();
  const vibeShown = await vibeBtn.waitFor({ timeout: 4000 }).then(
    () => true,
    () => false,
  );
  if (process.env.SMOKE_SHOTS) {
    await page.screenshot({ path: join(process.env.SMOKE_SHOTS, "t316-search.png") });
  }
  let waveOn = false;
  if (vibeShown) {
    await vibeBtn.click();
    for (let i = 0; i < 20 && !waveOn; i++) {
      await sleep(250);
      waveOn = (await invoke("playback_snapshot", {})).inWave;
    }
  }
  check(
    "T3.16: «что-то грустное и медленное» understood offline; «Играть этот вайб» starts the wave; nothing found → closest artists",
    JSON.stringify(vr.vibe?.chips) === JSON.stringify(["mood.sad", "tempo.slow"]) &&
      vibeOk &&
      vibeShown &&
      waveOn &&
      foundZ,
    `chips ${JSON.stringify(vr.vibe?.chips)}, tracks ${vibeIds.length} ok ${vibeOk}, button ${vibeShown}, wave ${waveOn}, suggestions ${JSON.stringify(sugg.suggestions.map((a) => a.name))} artists ${sugg.artists.length}`,
  );
  if ((await invoke("playback_snapshot", {})).state === "playing") {
    await invoke("playback_toggle", {});
  }

  // D-92 / AT-21 in the app: «вперёд» on a wave track re-picks the upcoming wave tracks at
  // once (the skip repels its vibe; whether a track is sad is judged in `sirtone-wave` tests).
  await invoke("wave_start", { profile: null, query: null });
  const upcoming = async () => {
    const q = await invoke("queue_get", {});
    return q.items.slice(q.position + 1).map((x) => x.trackId);
  };
  const upBefore = await upcoming();
  await invoke("playback_next", {});
  let upAfter = await upcoming();
  for (let i = 0; i < 20 && JSON.stringify(upAfter) === JSON.stringify(upBefore.slice(1)); i++) {
    await sleep(150);
    upAfter = await upcoming();
  }
  check(
    "D-92: a skip in the wave re-picks the upcoming wave tracks",
    upBefore.length >= 2 && JSON.stringify(upAfter) !== JSON.stringify(upBefore.slice(1)),
    `before ${JSON.stringify(upBefore)} → after ${JSON.stringify(upAfter)}`,
  );
  if ((await invoke("playback_snapshot", {})).state === "playing") {
    await invoke("playback_toggle", {});
  }

  // AT-20 (T3.32): «Для всех» — a track «не для компании» never comes, nothing sad or gloomy
  // is queued, and without popularity data the caption says so. (The guests' model leaving the
  // owner's models untouched is checked bit for bit in `sirtone-wave` tests.)
  const anal = (() => {
    const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    try {
      return db
        .prepare(
          "SELECT t.id, t.mood_bits FROM track t JOIN track_features tf ON tf.track_id = t.id WHERE t.state = 'ok' ORDER BY t.id",
        )
        .all();
    } finally {
      db.close();
    }
  })();
  const bannedId = anal[0]?.id;
  if (bannedId !== undefined) {
    await invoke("track_tag_set", { track: bannedId, tag: "not_for_company", on: true });
  }
  const fa = await invoke("wave_start", { profile: "for_all", query: null });
  const faQueue = (await invoke("queue_get", {})).items.map((x) => x.trackId);
  const bitsOf = new Map(anal.map((r) => [r.id, r.mood_bits]));
  const gloomy = faQueue.filter((id) => ((bitsOf.get(id) ?? 0) & (8 | 32)) !== 0);
  // The player's snapshot follows the engine's next tick.
  let faSnap = await invoke("playback_snapshot", {});
  for (let i = 0; i < 20 && faSnap.waveProfile !== "for_all"; i++) {
    await sleep(150);
    faSnap = await invoke("playback_snapshot", {});
  }
  await page.getByRole("navigation").getByRole("button", { name: "Моя волна" }).click();
  const faBtn = await page
    .getByRole("button", { name: "Для всех" })
    .first()
    .waitFor({ timeout: 3000 })
    .then(
      () => true,
      () => false,
    );
  check(
    "AT-20: «Для всех» — no «не для компании», nothing sad or gloomy, honest caption without popularity",
    fa.queued > 0 &&
      faSnap.waveProfile === "for_all" &&
      !faQueue.includes(bannedId) &&
      gloomy.length === 0 &&
      fa.noPopularity === true &&
      faBtn,
    `queued ${fa.queued} few ${fa.few} noPop ${fa.noPopularity}, profile ${faSnap.waveProfile}, banned ${bannedId} in ${faQueue.includes(bannedId)}, gloomy ${gloomy.length}, button ${faBtn}`,
  );
  if ((await invoke("playback_snapshot", {})).state === "playing") {
    await invoke("playback_toggle", {});
  }

  // AT-12 (T3.11, FR-IMP-007): a 3-hour mix lands in «Неизвестные» as «возможно, не трек»;
  // «нормальный» + an artist for it (the bulk action) moves it, and it goes into a playlist.
  await invoke("import_paths", { paths: [join(fixtures, "long", "mix-3h.mp3")] });
  const mixRow = () => {
    const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
    try {
      return db
        .prepare(
          "SELECT id, rel_path, identity_state, unknown_reason, user_normal FROM track WHERE rel_path LIKE '%mix-3h%' AND state = 'ok'",
        )
        .get();
    } finally {
      db.close();
    }
  };
  let mix = mixRow();
  for (let i = 0; i < 60 && !mix; i++) {
    await sleep(250);
    mix = mixRow();
  }
  const mixBefore = mix ? `${mix.identity_state}/${mix.unknown_reason}` : "none";
  let mixAfter = null;
  let inPl = false;
  if (mix) {
    await invoke("unknown_normal", { tracks: [mix.id] });
    const normalReason = mixRow()?.unknown_reason;
    const moved = await invoke("unknown_assign_artist", {
      tracks: [mix.id],
      artist: "Диджей Проверка",
    });
    mixAfter = { ...mixRow(), normalReason, moved };
    const roadPl12 = (await invoke("playlists_list", {})).find((p) => p.name === "Дорога");
    if (roadPl12) {
      await invoke("playlist_add", { id: roadPl12.id, tracks: [mix.id] });
      inPl = (await invoke("playlist_tracks", { id: roadPl12.id })).some(
        (e) => e.track.id === mix.id,
      );
    }
  }
  await page.getByRole("button", { name: "Неизвестные" }).click();
  const retryBtn = await page
    .getByRole("button", { name: "Повторить онлайн" })
    .waitFor({ timeout: 3000 })
    .then(
      () => true,
      () => false,
    );
  check(
    "AT-12: 3-hour mix «возможно, не трек» → «нормальный» + artist (bulk) → moved, in a playlist; «Повторить онлайн» offered",
    mixBefore === "unknown/possible_non_track" &&
      mixAfter?.normalReason === "needs_review" &&
      mixAfter?.moved === 1 &&
      mixAfter?.identity_state === "user" &&
      mixAfter?.user_normal === 1 &&
      mixAfter?.rel_path.startsWith("Диджей Проверка/") &&
      inPl &&
      retryBtn,
    `before ${mixBefore}, after ${JSON.stringify(mixAfter)}, playlist ${inPl}, retry ${retryBtn}`,
  );

  // T3.20 (FR-UI-006): albums without a cover get a generated placeholder (genre sign / logo
  // letter) instead of the grey note.
  const albumsNow = (await invoke("collection_albums", { after: null, limit: 500 })).rows;
  const bare = albumsNow.filter((a) => a.cover === null);
  await page.getByRole("navigation").getByRole("button", { name: "Альбомы" }).click();
  const placeholders = page.locator("[data-art]");
  for (let i = 0; i < 20 && (await placeholders.count()) === 0; i++) await sleep(150);
  const shown = await placeholders.count();
  if (process.env.SMOKE_SHOTS) {
    await page.screenshot({ path: join(process.env.SMOKE_SHOTS, "t320-albums.png") });
  }
  const genres = new Set(bare.map((a) => a.art >> 4));
  check(
    "T3.20: albums without a cover show a generated placeholder (genre / mood / logo)",
    bare.length > 0 &&
      bare.every((a) => typeof a.art === "number") &&
      shown > 0 &&
      // Fixture tracks carry no genre tags and synthetic sound: the base placeholder (genre 7);
      // the genre/mood variants are unit-tested (`art.rs`).
      [...genres].every((g) => g <= 7),
    `bare ${bare.length}, placeholders ${shown}, genres ${JSON.stringify([...genres])}`,
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
