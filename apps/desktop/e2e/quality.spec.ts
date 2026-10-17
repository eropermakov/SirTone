import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

/**
 * Stage 2 quality gates (T2.16, T2.18; `docs/08` §2) in the browser preview (no Tauri: the
 * library is empty, settings live in localStorage):
 *  - axe on every screen in both themes — no critical violations (serious ones are listed);
 *  - screenshot baselines: 2 themes × Sir.kov/Чёрный/Жёлтый/Бордовый × 3 widths, animations off.
 * Re-record baselines on purpose only: `pnpm e2e --update-snapshots`.
 */

const SCREENS = ["Моя волна", "Альбомы", "Исполнители", "Неизвестные", "Настройки"];

async function open(page: Page, theme: "dark" | "light", accent: string, width: number) {
  await page.addInitScript(
    ([t, a]) => {
      localStorage.setItem(
        "sirtone.web.settings",
        JSON.stringify({
          appearance: { theme: t, accent: { source: "preset", preset: a, custom: null } },
        }),
      );
    },
    [theme, accent] as const,
  );
  await page.setViewportSize({ width, height: 800 });
  await page.goto("/");
  await page.evaluate(() => document.fonts.ready);
}

for (const theme of ["dark", "light"] as const) {
  test(`axe: no critical violations (${theme})`, async ({ page }) => {
    await open(page, theme, "sirkov", 1280);
    const serious: string[] = [];
    for (const name of SCREENS) {
      await page.getByRole("button", { name, exact: true }).first().click();
      const r = await new AxeBuilder({ page }).analyze();
      const critical = r.violations.filter((v) => v.impact === "critical");
      expect(critical.map((v) => `${name}: ${v.id} — ${v.help}`)).toEqual([]);
      serious.push(
        ...r.violations.filter((v) => v.impact === "serious").map((v) => `${name}: ${v.id}`),
      );
    }
    test
      .info()
      .annotations.push({ type: "axe serious", description: serious.join(", ") || "none" });
  });
}

const ACCENTS = ["sirkov", "black", "yellow", "bordeaux"];
const WIDTHS = [1440, 1024, 860];

for (const theme of ["dark", "light"] as const) {
  for (const accent of ACCENTS) {
    test(`screens look as recorded: ${theme} / ${accent}`, async ({ page }) => {
      for (const width of WIDTHS) {
        await open(page, theme, accent, width);
        for (const name of ["Моя волна", "Настройки"]) {
          await page.getByRole("button", { name, exact: true }).first().click();
          await expect(page).toHaveScreenshot(`${theme}-${accent}-${width}-${name}.png`, {
            animations: "disabled",
            caret: "hide",
            maxDiffPixelRatio: 0.002,
          });
        }
      }
    });
  }
}
