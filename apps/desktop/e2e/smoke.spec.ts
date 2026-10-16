import { expect, test } from "@playwright/test";

test("shell renders with DESIGN.md tokens and Russian UI", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Моя волна" })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(page.locator("footer")).toHaveCSS("height", "84px");
  await expect(page.locator("nav")).toHaveCSS("width", "260px");
  const fontOk = await page.evaluate(async () => {
    await document.fonts.ready;
    return [...document.fonts].some((f) => f.family === "Geologica" && f.status === "loaded");
  });
  expect(fontOk).toBe(true);
});

test("navigation, theme and colour switch apply instantly", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Настройки" }).click();
  await page.getByRole("button", { name: "Светлая" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(page.locator("body")).toHaveCSS("background-color", "rgb(242, 238, 230)");
  await page.getByRole("button", { name: "Бордовый" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "bordeaux");
});

test("sidebar collapses below 900 px (DESIGN.md §7)", async ({ page }) => {
  await page.setViewportSize({ width: 860, height: 700 });
  await page.goto("/");
  await expect(page.locator("nav")).toHaveCSS("width", "64px");
});
