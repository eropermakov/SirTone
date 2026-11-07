// Cuts the promo film from the real-app scenes recorded by apps/desktop/e2e/promo-capture.mjs.
// Title cards and captions are rendered as HTML in Edge (Geologica, DESIGN.md tokens); the app
// footage sits in a rounded frame with the caption below it; the soundtrack is a synthesised pad
// (no third-party music). Output: target/promo/sirtone-promo.mp4
// node scripts/promo-assemble.mjs

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const require = createRequire(join(root, "apps", "desktop", "package.json"));
const { chromium } = require("@playwright/test");
const raw = join(root, "target", "promo", "raw");
const work = join(root, "target", "promo", "work");
const out = join(root, "target", "promo", "sirtone-promo.mp4");
mkdirSync(work, { recursive: true });
const FPS = 30;
const XF = 0.5; // crossfade, s
// Embedded: a page made with setContent may not load file:// fonts.
const fontFile = join(root, "apps", "desktop", "src", "assets", "fonts", "Geologica-Variable.ttf");
const font = `data:font/ttf;base64,${readFileSync(fontFile).toString("base64")}`;
/** The app footage sits in a rounded frame on the studio background; the caption goes below. */
const FRAME = { x: 240, y: 44, w: 1440, h: 810 };
/** Regions of a scene (in the 1920×1080 footage) blurred before framing. */
const HIDE = { colors: { x: 355, y: 893, w: 760, h: 64 } };

const ff = (args) => {
  const r = spawnSync("ffmpeg", ["-y", "-v", "error", ...args], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(r.stderr);
};
const dur = (f) =>
  Number(
    spawnSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", f], {
      encoding: "utf8",
    }).stdout,
  );

// —— Cards, stage, mask and captions (HTML → PNG) ——
const CSS = `
@font-face { font-family: G; src: url("${font}"); font-weight: 100 900; }
* { margin: 0; box-sizing: border-box; }
body { width: 1920px; height: 1080px; font-family: G; color: #f3ece2; }
.card { width: 100%; height: 100%; background: radial-gradient(120% 90% at 30% 20%, #201918 0%, #110d0c 60%);
  display: flex; flex-direction: column; justify-content: center; padding: 0 180px; gap: 28px; }
.kicker { font-weight: 600; font-size: 34px; color: #ccff00; letter-spacing: .02em; }
.big { font-weight: 700; font-size: 120px; line-height: 1.04; letter-spacing: -.02em; }
.mid { font-weight: 400; font-size: 44px; color: #b8aea6; }
.bar { width: 120px; height: 10px; border-radius: 9999px; background: #ccff00; }
.plate { display: inline-block; background: #5e0a18; color: #f3ece2; border-radius: 9999px;
  padding: 10px 26px; font-weight: 600; font-size: 30px; }
.cap { position: absolute; left: 0; right: 0; top: 884px; display: flex; flex-direction: column;
  align-items: center; gap: 8px; text-align: center; }
.cap b { font-weight: 700; font-size: 52px; line-height: 1.1; letter-spacing: -.01em; }
.cap span { font-weight: 400; font-size: 30px; color: #b8aea6; }
.bg { width: 100%; height: 100%; background: radial-gradient(120% 90% at 50% 0%, #201918 0%, #110d0c 65%); }
.frame { position: absolute; left: ${FRAME.x - 1}px; top: ${FRAME.y - 1}px; width: ${FRAME.w + 2}px;
  height: ${FRAME.h + 2}px; border-radius: 21px; border: 1px solid rgba(243,236,226,.14);
  box-shadow: 0 24px 64px rgba(0,0,0,.55); }
.mask { width: ${FRAME.w}px; height: ${FRAME.h}px; border-radius: 20px; background: #fff; }
`;
const CARDS = {
  hook: `<div class="card"><div class="kicker">Ваша музыка</div><div class="big">Без стриминга.<br>Без интернета.</div><div class="mid">Только ваши файлы — и плеер, который их понимает.</div></div>`,
  end: `<div class="card"><div class="bar"></div><div class="big">Сиртон</div><div class="mid">Локальный музыкальный плеер для Windows</div><div><span class="plate">Sir.kov Studio</span></div></div>`,
};
const CAPTIONS = {
  wave: ["Моя волна", "Подбирает музыку под вас — офлайн, на вашем ПК"],
  collection: ["Медиатека раскладывается сама", "Исполнители, альбомы, обложки — из ваших файлов"],
  tune: ["Под занятие и настроение", "Волна настраивается в пару касаний"],
  colors: ["17 цветов, две темы", "Цвет меняется сразу и перетекает за 400 мс"],
  light: ["«Утренняя бумага»", "Светлая тема — тёплая, как бумага"],
  viz: ["Визуализация во весь экран", "Музыку видно"],
};
const browser = await chromium.launch({ channel: "msedge" });
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
const render = async (html, path, opts = {}) => {
  await page.setContent(`<style>${CSS}${opts.css ?? ""}</style>${html}`);
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path, omitBackground: !!opts.transparent });
};
for (const [k, html] of Object.entries(CARDS)) await render(html, join(work, `card-${k}.png`));
await render(`<div class="bg"></div><div class="frame"></div>`, join(work, "stage.png"));
for (const [k, [t, s]] of Object.entries(CAPTIONS))
  await render(`<div class="cap"><b>${t}</b><span>${s}</span></div>`, join(work, `cap-${k}.png`), {
    css: "body{background:transparent}",
    transparent: true,
  });
await page.setViewportSize({ width: FRAME.w, height: FRAME.h });
await render(`<div class="mask"></div>`, join(work, "mask.png"), {
  css: "body{width:auto;height:auto;background:#000}",
});
await browser.close();

// —— Segments ——
const ORDER = ["hook", "wave", "collection", "tune", "colors", "light", "viz", "end"];
const segs = [];
for (const k of ORDER) {
  const f = join(work, `seg-${k}.mp4`);
  if (CARDS[k]) {
    const d = k === "hook" ? 3.2 : 4.2;
    // A slow push-in on the card.
    ff([
      "-framerate",
      String(FPS),
      "-loop",
      "1",
      "-t",
      String(d),
      "-i",
      join(work, `card-${k}.png`),
      "-vf",
      `scale=2112:-2,zoompan=z='1+0.035*on/(${d}*${FPS})':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s=1920x1080:fps=${FPS},format=yuv420p`,
      "-c:v",
      "libx264",
      "-preset",
      "medium",
      "-crf",
      "17",
      f,
    ]);
  } else {
    const list = join(raw, k, "list.ffconcat");
    if (!existsSync(list)) {
      console.log(`skip ${k}: no footage`);
      continue;
    }
    // Caption fades in at 0.5 s and out 0.5 s before the end.
    const d0 = readFileSync(list, "utf8")
      .split("\n")
      .filter((l) => l.startsWith("duration "))
      .reduce((a, l) => a + Number(l.slice(9)), 0);
    const fit =
      k === "viz"
        ? // Full screen: the window grows past the emulated viewport; keep the picture, fill 16:9.
          "crop=iw:ih*0.93:0:0,scale=1920:1080:force_original_aspect_ratio=increase,crop=1920:1080"
        : "scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:-1:-1:color=0x110d0c";
    // The settings show the library path (a temp folder under the Windows user name): blurred.
    const h = HIDE[k];
    const hide = h
      ? `,split[s0][s1];[s1]crop=${h.w}:${h.h}:${h.x}:${h.y},boxblur=14:2[bl];[s0][bl]overlay=${h.x}:${h.y}`
      : "";
    ff([
      "-f",
      "concat",
      "-safe",
      "0",
      "-i",
      list,
      "-loop",
      "1",
      "-i",
      join(work, "stage.png"),
      "-loop",
      "1",
      "-i",
      join(work, `cap-${k}.png`),
      "-loop",
      "1",
      "-i",
      join(work, "mask.png"),
      "-filter_complex",
      `[0:v]fps=${FPS},${fit}${hide},scale=${FRAME.w}:${FRAME.h},format=rgba,setsar=1[f];` +
        `[3:v]format=gray,scale=${FRAME.w}:${FRAME.h}[m];[f][m]alphamerge[fr];` +
        `[1:v][fr]overlay=${FRAME.x}:${FRAME.y}:shortest=1[b];` +
        `[2:v]format=rgba,fade=t=in:st=0.5:d=0.4:alpha=1,fade=t=out:st=${Math.max(1.2, d0 - 0.9)}:d=0.4:alpha=1[c];` +
        `[b][c]overlay=0:0:shortest=1,format=yuv420p[o]`,
      "-map",
      "[o]",
      "-t",
      String(d0),
      "-c:v",
      "libx264",
      "-preset",
      "medium",
      "-crf",
      "17",
      f,
    ]);
  }
  segs.push({ k, f, d: dur(f) });
}

// —— Crossfaded cut ——
// Same timebase for every input, or xfade refuses.
let fc = segs.map((_s, i) => `[${i}:v]settb=AVTB,fps=${FPS}[n${i}];`).join("");
let last = "[n0]";
let offset = 0;
segs.forEach((_s, i) => {
  if (i === 0) return;
  offset += segs[i - 1].d - XF;
  const o = i === segs.length - 1 ? "[vout]" : `[x${i}]`;
  fc += `${last}[n${i}]xfade=transition=fade:duration=${XF}:offset=${offset.toFixed(3)}${o};`;
  last = o;
});
const total = segs.reduce((a, s) => a + s.d, 0) - XF * (segs.length - 1);

// —— Soundtrack: a soft synthesised pad, Am – F – C – G, 2.5 s a chord ——
const CH = [
  [220, 261.63, 329.63, 110],
  [174.61, 220, 261.63, 87.31],
  [196, 261.63, 329.63, 130.81],
  [196, 246.94, 293.66, 98],
];
const chordFiles = CH.map((c, i) => {
  const f = join(work, `chord${i}.wav`);
  const expr = c
    .map(
      (hz, j) =>
        `${j === 3 ? 0.22 : 0.16}*sin(2*PI*${hz}*t)*(0.85+0.15*sin(2*PI*${0.3 + j * 0.07}*t))`,
    )
    .join("+");
  ff([
    "-f",
    "lavfi",
    "-i",
    `aevalsrc='${expr}':s=48000:d=2.5`,
    "-af",
    "afade=t=in:d=0.25,afade=t=out:st=2.15:d=0.35",
    f,
  ]);
  return f;
});
const loops = Math.ceil(total / 10) + 1;
const concatList = join(work, "chords.txt");
writeFileSync(
  concatList,
  Array.from({ length: loops }, () =>
    chordFiles.map((f) => `file '${f.replace(/\\/g, "/")}'`).join("\n"),
  ).join("\n"),
);
const music = join(work, "music.wav");
ff([
  "-f",
  "concat",
  "-safe",
  "0",
  "-i",
  concatList,
  "-af",
  `lowpass=f=2400,aecho=0.8:0.7:180|360:0.35|0.22,volume=0.9,afade=t=in:d=1.2,afade=t=out:st=${(total - 2.2).toFixed(2)}:d=2.2,atrim=0:${total.toFixed(2)}`,
  "-ac",
  "2",
  music,
]);

ff([
  ...segs.flatMap((s) => ["-i", s.f]),
  "-i",
  music,
  "-filter_complex",
  fc.slice(0, -1),
  "-map",
  "[vout]",
  "-map",
  `${segs.length}:a`,
  "-c:v",
  "libx264",
  "-preset",
  "slow",
  "-crf",
  "18",
  "-pix_fmt",
  "yuv420p",
  "-movflags",
  "+faststart",
  "-c:a",
  "aac",
  "-b:a",
  "160k",
  "-shortest",
  out,
]);
console.log(`promo: ${out} — ${total.toFixed(1)} s`);
