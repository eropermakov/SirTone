// T7.6 beta on the owner's music — from the COPY only (`D:\SirVault\SirTone-owner-copy`), never
// from the live `%LOCALAPPDATA%\SirTone` or `Music\SirTone`. The copy's audio files are copied
// once more into a temp folder (the copy itself is never an import source), imported into an
// isolated instance (`SIRTONE_DATA_DIR`, its own library folder), analysed to the end; then a
// report for a person to read: what landed where, «Неизвестные» and why, artists and albums as
// the folders show them, duplicates, languages, analysis coverage, the wave's first tracks, and
// every WARN/ERROR line of the log. Screenshots of the main screens go next to it.
// BETA_SRC=D:\SirVault\SirTone-owner-copy\library SMOKE_PORT=9374 node apps/desktop/e2e/beta-owner.mjs

import { execFileSync, spawn } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { createRequire } from "node:module";
import { extname, join, relative } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { closeMainScript } from "./close-main.mjs";

const require = createRequire(import.meta.url);
const { chromium } = require("@playwright/test");
const root = join(import.meta.dirname, "..", "..", "..");
const exe = join(root, "target", "release", "sirtone-app.exe");
const SRC = process.env.BETA_SRC ?? "D:\\SirVault\\SirTone-owner-copy\\library";
if (/\\Music\\SirTone$|\\AppData\\Local\\SirTone/i.test(SRC)) throw new Error("live folder: no");
const PORT = Number(process.env.SMOKE_PORT ?? 9374);
const outDir = join(root, "target", "beta");
mkdirSync(join(outDir, "shots"), { recursive: true });
// On D: next to the report: a run copies the music in twice (≈ 1 GB), and C: is nearly full.
const dataDir = mkdtempSync(join(outDir, "data-"));
const libDir = join(dataDir, "Music", "SirTone");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const lines = [];
const out = (s = "") => {
  console.log(s);
  lines.push(s);
};

// The copy's audio, as a person would drop it: same folders, no `.sirtone`.
const AUDIO = new Set([".mp3", ".flac", ".m4a", ".ogg", ".opus", ".wav", ".wma", ".aac", ".aiff"]);
const inDir = join(dataDir, "in");
let copied = 0;
const copyTree = (from, to) => {
  for (const e of readdirSync(from, { withFileTypes: true })) {
    if (e.name === ".sirtone") continue;
    const s = join(from, e.name);
    const d = join(to, e.name);
    if (e.isDirectory()) copyTree(s, d);
    else if (AUDIO.has(extname(e.name).toLowerCase())) {
      mkdirSync(to, { recursive: true });
      copyFileSync(s, d);
      copied++;
    }
  }
};
copyTree(SRC, inDir);

const app = spawn(exe, [], {
  env: {
    ...process.env,
    SIRTONE_DATA_DIR: dataDir,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
  },
  stdio: "ignore",
});
const ps = (script) =>
  execFileSync("powershell", ["-NoProfile", "-Command", script], { encoding: "utf8" }).trim();

let browser;
try {
  let page;
  for (let i = 0; i < 120 && !page; i++) {
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${PORT}`);
      page = browser
        .contexts()
        .flatMap((c) => c.pages())
        .find((x) => !x.url().includes("widget"));
    } catch {
      await sleep(250);
    }
  }
  if (!page) throw new Error("no page");
  await page.waitForSelector("footer", { timeout: 30000 });
  const invoke = (cmd, args = {}) =>
    page.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
  await invoke("settings_patch", { patch: { onboarding: { done: true, tips_seen: true } } });
  await page.reload();
  await page.waitForSelector("footer");
  await invoke("playback_set_volume", { volume: 0.05 });

  const t0 = Date.now();
  await invoke("import_paths", { paths: [inDir] });
  await page.getByRole("button", { name: "Продолжить" }).click();
  await page
    .getByRole("status")
    .filter({ hasText: "Музыка добавлена" })
    .waitFor({ timeout: 15 * 60000 });
  const importS = (Date.now() - t0) / 1000;
  if (process.env.BETA_SHOTS !== "0") {
    await page.screenshot({ path: join(outDir, "shots", "01-imported.png") });
  }
  // Analysis to the end (or 40 min).
  let an = { done: 0, total: 1 };
  for (let i = 0; i < 480; i++) {
    an = await invoke("analysis_progress");
    if (an.total > 0 && an.done >= an.total) break;
    await sleep(5000);
  }
  const analyseS = (Date.now() - t0) / 1000 - importS;
  const status = await invoke("library_status");

  out(`# Бета на копии медиатеки владельца — ${new Date().toISOString()}`);
  out();
  out(`Источник: ${SRC} (скопировано файлов: ${copied})`);
  out(
    `Импорт: ${importS.toFixed(0)} с · анализ: ${analyseS.toFixed(0)} с (${an.done}/${an.total}${an.paused ? ", на паузе" : ""})`,
  );
  out(
    `Медиатека: треков ${status.tracks}, исполнителей ${status.artists}, альбомов ${status.albums}, в «Неизвестных» ${status.unknown}, ${(status.bytes / 1048576).toFixed(0)} МБ`,
  );

  const db = new DatabaseSync(join(libDir, ".sirtone", "library.db"), { readOnly: true });
  const q = (sql, ...a) => db.prepare(sql).all(...a);
  out();
  out("## Не добавлено");
  const batches = q("SELECT id FROM import_batch ORDER BY id");
  for (const b of batches) {
    const r = await invoke("import_report", { batch: b.id });
    for (const x of r.rejected) out(`- ${relative(inDir, x.path)} — ${x.reason}`);
    for (const d of r.duplicates) out(`- точная копия: ${relative(inDir, d)}`);
  }
  out();
  out("## «Неизвестные»");
  for (const u of await invoke("unknown_list")) {
    const cand = u.candidates
      .map((c) => `${c.artists} — ${c.title}${c.online ? " (MB)" : ""}`)
      .join("; ");
    out(
      `- ${u.track.title} (${u.track.artists ?? "?"}) — ${u.reason ?? "?"}${cand ? ` · варианты: ${cand}` : ""}`,
    );
  }
  out();
  out("## Раскладка по папкам (исполнитель / альбом / файл)");
  for (const r of q(
    "SELECT rel_path, identity_state, identity_conf FROM track ORDER BY rel_path",
  )) {
    out(`- ${r.rel_path} · ${r.identity_state} ${r.identity_conf?.toFixed(2) ?? ""}`);
  }
  out();
  out("## Исполнители");
  out(
    q(
      `SELECT a.name, count(*) n FROM artist a JOIN track_artist ta ON ta.artist_id = a.id
       GROUP BY a.id ORDER BY a.name`,
    )
      .map((r) => `${r.name} (${r.n})`)
      .join(" · "),
  );
  out();
  out("## Языки, анализ, громкость");
  for (const r of q("SELECT coalesce(lang, '—') l, count(*) n FROM track GROUP BY 1"))
    out(`- язык ${r.l}: ${r.n}`);
  const cov = q(
    `SELECT count(bpm) bpm, count(key_camelot) k, count(energy) e, count(embedding) emb,
            count(lufs_i) lufs, count(fingerprint) fp, count(*) n FROM track`,
  )[0];
  out(
    `- темп ${cov.bpm}/${cov.n}, тональность ${cov.k}, энергия ${cov.e}, эмбеддинг ${cov.emb}, LUFS ${cov.lufs}, отпечаток ${cov.fp}`,
  );
  const dups = await invoke("duplicates_list");
  out();
  out("## Дубли");
  for (const p of dups.pairs ?? [])
    out(
      `- ${p.class}${p.similarity != null ? ` ${p.similarity.toFixed(2)}` : ""}: ${p.a.track.title} ↔ ${p.b.track.title}`,
    );
  if (!(dups.pairs ?? []).length) out("- нет");

  // The wave: 12 tracks.
  await invoke("wave_start", { profile: null, query: null });
  await sleep(3000);
  const queue = await invoke("queue_get");
  out();
  out("## «Моя волна» — первые треки");
  for (const it of queue.items.slice(0, 12)) out(`- ${it.title} — ${it.artists ?? "?"}`);
  for (const [i, name] of ["Моя волна", "Исполнители", "Альбомы", "Неизвестные"].entries()) {
    await page.getByRole("navigation").getByRole("button", { name }).first().click();
    await sleep(1500);
    await page.screenshot({ path: join(outDir, "shots", `0${i + 2}-${name}.png`) });
  }
  await invoke("playback_toggle");
  db.close();

  // Every warning and error the app logged.
  out();
  out("## Журнал: WARN и ERROR");
  const logs = join(dataDir, "logs");
  const bad = existsSync(logs)
    ? readdirSync(logs)
        .filter((f) => f.startsWith("sirtone."))
        .flatMap((f) => readFileSync(join(logs, f), "utf8").split(/\r?\n/))
        .filter((l) => /\b(WARN|ERROR)\b/.test(l))
    : [];
  for (const l of bad.slice(0, 80)) out(`- ${l.slice(0, 300)}`);
  if (!bad.length) out("- нет");
  out();
  out(`Данные экземпляра (оставлены для разбора): ${dataDir}`);
} finally {
  writeFileSync(join(outDir, "report.md"), `${lines.join("\n")}\n`);
  await browser?.close().catch(() => {});
  try {
    ps(closeMainScript(app.pid));
  } catch {}
  await sleep(2000);
  // Only this test's own process, by its PID.
  try {
    process.kill(app.pid);
  } catch {}
}
