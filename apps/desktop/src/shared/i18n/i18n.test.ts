import { describe, expect, it } from "vitest";
import { createI18n, namespaces, resources } from "./index";

describe("i18n", () => {
  it("uses Russian plural forms one/few/many", async () => {
    const i18n = await createI18n("ru");
    expect(i18n.t("count.tracks", { count: 1 })).toBe("1 трек");
    expect(i18n.t("count.tracks", { count: 2 })).toBe("2 трека");
    expect(i18n.t("count.tracks", { count: 5 })).toBe("5 треков");
    expect(i18n.t("count.tracks", { count: 21 })).toBe("21 трек");
  });

  it("falls back to Russian for the empty English slot", async () => {
    const i18n = await createI18n("en");
    expect(i18n.t("nav:wave")).toBe("Моя волна");
  });

  it("the English slot has no key Russian lacks (T6.5)", () => {
    const keys = (o: object, p = ""): string[] =>
      Object.entries(o).flatMap(([k, v]) =>
        v && typeof v === "object" ? keys(v as object, `${p}${k}.`) : [`${p}${k}`],
      );
    for (const ns of namespaces) {
      const ru = new Set(keys(resources.ru[ns]));
      const stray = keys(resources.en[ns]).filter((k) => !ru.has(k));
      expect(stray, ns).toEqual([]);
    }
  });

  it("English plurals use the English rule", async () => {
    const i18n = await createI18n("en");
    i18n.addResource("en", "common", "test.n", "{count, plural, one {# track} other {# tracks}}");
    expect(i18n.t("test.n", { count: 1 })).toBe("1 track");
    expect(i18n.t("test.n", { count: 2 })).toBe("2 tracks");
  });
});
