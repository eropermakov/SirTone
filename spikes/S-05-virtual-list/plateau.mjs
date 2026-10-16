// Does renderer memory plateau while scrolling unique covers for a long time?
import { execSync } from "node:child_process";
import { chromium } from "@playwright/test";
const edgeMb = () => Math.round(Number(execSync(`powershell -NoProfile -Command "(Get-CimInstance Win32_Process -Filter \\"Name='msedge.exe'\\" | Where-Object { $_.CommandLine -like '*playwright*' } | ForEach-Object { (Get-Process -Id $_.ProcessId).PrivateMemorySize64 } | Measure-Object -Sum).Sum / 1MB"`).toString().trim().replace(",", ".")));
const browser = await chromium.launch({ channel: "msedge", headless: false });
const page = await browser.newPage({ viewport: { width: 1548, height: 860 } });
await page.goto("about:blank"); await page.waitForTimeout(1500);
const base = edgeMb();
await page.goto(`http://127.0.0.1:1430/?mode=${process.argv[2] ?? "list&cover=40&dir=covers80"}`);
await page.waitForSelector("#scroller img");
const line = [];
for (let s = 1; s <= 8; s++) {
  await page.evaluate(() => new Promise((done) => { const el = document.getElementById("scroller"); let last = performance.now(); const t0 = last;
    const tick = (now) => { el.scrollTop += 20000 * (now - last) / 1000; last = now; if (now - t0 < 7500) requestAnimationFrame(tick); else done(); }; requestAnimationFrame(tick); }));
  line.push(`${s * 7.5}s:+${edgeMb() - base}`);
}
await page.goto("about:blank"); await page.waitForTimeout(3000);
console.log(`base ${base} MB; ${line.join("  ")}; back to blank: +${edgeMb() - base}`);
await browser.close();
