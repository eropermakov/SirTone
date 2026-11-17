import { expect, test } from "@playwright/test";

/** The compact landscape mode is intended for a phone-sized WebView or narrow display. */
test("compact landscape keeps the player visible and touch-sized", async ({ page }) => {
  await page.setViewportSize({ width: 800, height: 390 });
  await page.goto("/");
  await page.evaluate(() => document.fonts.ready);

  const player = page.locator("footer");
  await expect(player).toBeVisible();
  await expect(player).toHaveCSS("height", "64px");
  await expect(player.locator("button").first()).toHaveCSS("height", "36px");
  expect((await player.screenshot()).byteLength).toBeGreaterThan(1_000);
});
