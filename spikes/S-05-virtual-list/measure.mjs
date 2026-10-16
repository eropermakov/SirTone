// Drives the spike page in Edge (WebView2 = Chromium/Edge engine) and measures frame times while
// scrolling at fixed speeds, plus JS heap and renderer memory. Usage: start `pnpm dev`, then
// `node measure.mjs`.
import { execSync } from "node:child_process";
import { chromium } from "@playwright/test";

const browser = await chromium.launch({ channel: "msedge", headless: false, args: ["--window-size=1600,1000"] });
const page = await browser.newPage({ viewport: { width: 1548, height: 860 } });
const cdp = await page.context().newCDPSession(page);
await cdp.send("Performance.enable");

const edgeMb = () =>
  Math.round(
    Number(
      execSync(
        `powershell -NoProfile -Command "(Get-CimInstance Win32_Process -Filter \\"Name='msedge.exe'\\" | Where-Object { $_.CommandLine -like '*playwright*' } | ForEach-Object { (Get-Process -Id $_.ProcessId).PrivateMemorySize64 } | Measure-Object -Sum).Sum / 1MB"`,
      )
        .toString()
        .trim()
        .replace(",", "."),
    ),
  );
await page.goto("about:blank");
await page.waitForTimeout(1500);
const baseline = edgeMb();

async function run(url, label, pxPerSec, seconds) {
  await page.goto(url);
  await page.waitForSelector("#scroller img");
  await page.waitForTimeout(500);
  const r = await page.evaluate(
    ([speed, secs]) =>
      new Promise((done) => {
        const el = document.getElementById("scroller");
        const deltas = [];
        let last = performance.now();
        const start = last;
        const tick = (now) => {
          deltas.push(now - last);
          last = now;
          el.scrollTop += (speed * deltas[deltas.length - 1]) / 1000;
          if (now - start < secs * 1000) requestAnimationFrame(tick);
          else done({ deltas: deltas.slice(5), dist: el.scrollTop });
        };
        requestAnimationFrame(tick);
      }),
    [pxPerSec, seconds],
  );
  const d = r.deltas.sort((a, b) => a - b);
  const avg = d.reduce((s, x) => s + x, 0) / d.length;
  const p95 = d[Math.floor(d.length * 0.95)];
  const long = d.filter((x) => x > 25).length;
  const m = Object.fromEntries((await cdp.send("Performance.getMetrics")).metrics.map((x) => [x.name, x.value]));
  console.log(
    `| ${label} | ${pxPerSec} px/s | ${(1000 / avg).toFixed(1)} | ${p95.toFixed(1)} | ${long}/${d.length} | ${(m.JSHeapUsedSize / 1e6).toFixed(0)} MB | ${m.Nodes} |`,
  );
}

const scenario = process.argv[2] ?? "list&cover=40";
console.log(`scenario ${scenario}`);
for (const speed of [1500, 6000, 30000]) await run(`http://127.0.0.1:1430/?mode=${scenario}`, scenario, speed, 8);
const after = edgeMb();
console.log(`
Edge private memory (all processes): blank page ${baseline} MB, after list+grid runs ${after} MB, delta ${after - baseline} MB`);
await browser.close();
