import { describe, expect, it } from "vitest";
import { createI18n } from "./index";

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
});
