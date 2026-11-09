import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
// @ts-expect-error — plain .mjs generator (T9.1), no types
import { build, parseTokens, slugs } from "../../../../../scripts/gen-tokens.mjs";
import { ACCENT_SLUGS } from "../theme/theme";
import { GLASS } from "../ui/tokens/tokens.generated";
import { BACKGROUNDS, contrast, ON_PRIMARY } from "./color";

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");
const design = read("../../../../../DESIGN.md");
const seeds = JSON.parse(read("../theme/accentSeeds.json"));
const out = build(design, seeds) as { css: string; ts: string };
const tokens = parseTokens(design);
const hexes = (o: Record<string, string>) => Object.values(o).filter((v) => v.startsWith("#"));

describe("tokens generated from DESIGN.md (T9.1, FR-LOOK-003)", () => {
  it("sirtone.css matches DESIGN.md — run `pnpm --filter @sirtone/desktop gen:tokens`", () => {
    expect(read("../ui/tokens/sirtone.css")).toBe(out.css);
  });
  it("tokens.generated.ts matches DESIGN.md", () => {
    expect(read("../ui/tokens/tokens.generated.ts")).toBe(out.ts);
  });
  it("every colour of the four themes appears in the CSS", () => {
    const css = out.css.toLowerCase();
    for (const theme of ["light", "dark", "lightHC", "darkHC"] as const) {
      for (const h of hexes(tokens.color[theme]))
        expect(css, `${theme} ${h}`).toContain(h.toLowerCase());
    }
  });
  it("the accent is #650A12 in both base themes; Fabric is not emitted (D-155)", () => {
    expect(tokens.color.dark.accent).toBe("#650A12");
    expect(tokens.color.light.accent).toBe("#650A12");
    expect(out.css.toLowerCase()).not.toContain(tokens.color.fabric.accent.toLowerCase());
    expect(out.css).not.toContain("fabric");
  });
  it("the slug list of the app is the 17 sets of the generator", () => {
    expect([...ACCENT_SLUGS].sort()).toEqual(slugs(seeds).sort());
  });
});

describe("contrast of the new neutrals (DESIGN.md §4, Q-32)", () => {
  for (const theme of ["dark", "light", "darkHC", "lightHC"] as const) {
    const c = tokens.color[theme];
    const surfaces = [c.bg, c.surface, c.raised, c.flyout];
    it(`${theme}: text, muted text and danger ≥ 4.5:1 on every surface`, () => {
      for (const k of ["text", "textMuted", "danger", "success", "warning"] as const) {
        for (const bg of surfaces) {
          expect(contrast(c[k], bg), `${k} on ${bg}`).toBeGreaterThanOrEqual(4.5);
        }
      }
    });
    it(`${theme}: text on the neutral plate ≥ 7:1; content on the accent fill ≥ 4.5:1`, () => {
      expect(contrast(c.onPrimary, c.primary)).toBeGreaterThanOrEqual(7);
      expect(contrast(c.onAccent, c.accent)).toBeGreaterThanOrEqual(4.5);
    });
    it(`${theme}: accentText (thin lines) ≥ 4.5:1 on every surface`, () => {
      for (const bg of surfaces) expect(contrast(c.accentText, bg)).toBeGreaterThanOrEqual(4.5);
    });
  }
  it("exported backgrounds follow the tokens", () => {
    expect(BACKGROUNDS.dark[0]).toBe(tokens.color.dark.bg);
    expect(ON_PRIMARY.light).toBe(tokens.color.light.onPrimary);
  });
});

describe("glass levels (D-156, T9.5)", () => {
  it("DESIGN.md sets 55 / 78 / 97 % and 26 px; the CSS never goes below the set level", () => {
    expect(tokens.glass).toMatchObject({ transparent: 55, medium: 78, matte: 97, blur: 26 });
    expect(out.css).toContain("--glass-blur: 26px;");
    expect(GLASS.blur).toBe(26);
    for (const level of ["transparent", "medium", "matte"] as const) {
      for (const theme of ["dark", "light"] as const) {
        const l = GLASS.levels[level][theme];
        expect(l.effective).toBeGreaterThanOrEqual(l.set);
        // `text` on the panel over the worst backdrop stays ≥ 4.5:1 at the level in use.
        const c = tokens.color[theme];
        const worst = theme === "dark" ? "#ffffff" : "#000000";
        const mix = (a: string, b: string, t: number) =>
          `#${[1, 3, 5]
            .map((i) =>
              Math.round(
                Number.parseInt(a.slice(i, i + 2), 16) * t +
                  Number.parseInt(b.slice(i, i + 2), 16) * (1 - t),
              )
                .toString(16)
                .padStart(2, "0"),
            )
            .join("")}`;
        expect(contrast(c.text, mix(c.bg, worst, l.effective / 100))).toBeGreaterThanOrEqual(4.45);
      }
    }
  });
});
