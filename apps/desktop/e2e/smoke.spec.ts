import { expect, test } from "@playwright/test";

test("shell renders with DESIGN.md tokens and Russian UI", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Моя волна" })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(page.locator("footer")).toHaveCSS("height", "84px");
  await expect(page.locator("nav")).toHaveCSS("width", "280px");
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
  await expect(page.locator("body")).toHaveCSS("background-color", "rgb(244, 244, 244)");
  await page.getByRole("button", { name: "Зелёный" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "green");
});

test("sidebar collapses below 900 px (DESIGN.md §7)", async ({ page }) => {
  await page.setViewportSize({ width: 860, height: 700 });
  await page.goto("/");
  await expect(page.locator("nav")).toHaveCSS("width", "84px");
});

test("two themes, monochrome with the #650A12 accent (D-155, FR-LOOK-001/002)", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Настройки" }).click();
  const html = page.locator("html");
  await page.getByRole("button", { name: "Тёмная" }).click();
  await expect(html).toHaveAttribute("data-theme", "dark");
  await expect(page.locator("body")).toHaveCSS("background-color", "rgb(0, 0, 0)");
  await page.getByRole("button", { name: "Светлая" }).click();
  await expect(html).toHaveAttribute("data-theme", "light");
  await expect(page.locator("body")).toHaveCSS("background-color", "rgb(244, 244, 244)");
  // The removed owner palettes and Fabric are gone (D-155).
  await expect(page.getByRole("button", { name: "Оранжевая" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Чёрно-белая" })).toHaveCount(0);
  await expect(html).not.toHaveAttribute("data-palette", /./);
});

test("performance presets switch motion at once (T2.3)", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Настройки" }).click();
  const html = page.locator("html");
  await page.getByRole("button", { name: "Очень слабое" }).click();
  await expect(html).toHaveAttribute("data-motion", "0");
  await expect(html).toHaveAttribute("data-blur", "off");
  await page.getByRole("button", { name: "Супермощное" }).click();
  await expect(html).toHaveAttribute("data-motion", "1.5");
  await page.getByRole("switch", { name: "Анимации интерфейса" }).click();
  await expect(html).toHaveAttribute("data-motion", "0");
});

test("glass: three levels, panels follow the level (D-156, FR-LOOK-007)", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Настройки" }).click();
  const html = page.locator("html");
  for (const [name, level] of [
    ["Матовое", "matte"],
    ["Прозрачное", "transparent"],
    ["Среднее", "medium"],
  ] as const) {
    await page.getByRole("group", { name: "Стекло" }).getByRole("button", { name }).click();
    await expect(html).toHaveAttribute("data-glass", level);
  }
  await expect(page.locator("nav")).toHaveClass(/glass/);
  await expect(page.locator("footer")).toHaveClass(/glass/);
});

test("motion: one curve, and «уменьшить движение» leaves only opacity (DESIGN.md §9, FR-LOOK-010)", async ({
  browser,
}) => {
  const normal = await browser.newPage();
  await normal.goto("/");
  const nav = normal.locator("nav button", { hasText: "Исполнители" });
  const curve = await nav.evaluate((el) => getComputedStyle(el).transitionTimingFunction);
  expect(curve).toBe("cubic-bezier(0, 0, 0, 1)");
  // The duration is the token of the performance level (--dur-fast), never a literal.
  const [ms, token] = await nav.evaluate((el) => {
    const toMs = (v: string) =>
      v.trim().endsWith("ms") ? Number.parseFloat(v) : Number.parseFloat(v) * 1000;
    return [
      toMs(getComputedStyle(el).transitionDuration),
      toMs(getComputedStyle(document.documentElement).getPropertyValue("--dur-fast")),
    ];
  });
  expect(ms).toBeCloseTo(token as number, 0);
  await normal.close();
  const calm = await browser.newPage({ reducedMotion: "reduce" });
  await calm.goto("/");
  const cn = calm.locator("nav button", { hasText: "Исполнители" });
  expect(await cn.evaluate((el) => getComputedStyle(el).transitionProperty)).toBe("opacity");
  await calm.close();
});
