import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { BACKGROUNDS, contrast, generateAccentSet, INK_LIGHT, type Theme } from "./color";

const tokensCss = readFileSync(new URL("../ui/tokens/sirtone.css", import.meta.url), "utf8");

type Set = Record<"accent-fill" | "accent-text" | "accent-on" | "primary" | "primary-text", string>;

/** Parses the 17 owner sets × 2 themes from DESIGN.md §13 CSS (copied verbatim into sirtone.css). */
function parseSets(): Map<string, Set> {
  const sets = new Map<string, Set>();
  const re = /\[data-theme="(dark|light)"\]\[data-accent="([a-z]+)"\][^{]*\{([^}]*)\}/g;
  for (const m of tokensCss.matchAll(re)) {
    const vars = Object.fromEntries(
      [...(m[3] ?? "").matchAll(/--([a-z-]+):\s*(#[0-9a-f]{6})/g)].map((v) => [v[1], v[2]]),
    ) as Set;
    sets.set(`${m[1]}:${m[2]}`, vars);
  }
  return sets;
}

describe("DESIGN.md colour sets (T0.9)", () => {
  const sets = parseSets();

  it("has 17 sets for each theme", () => {
    expect(sets.size).toBe(34);
  });

  for (const [key, s] of parseSets()) {
    const [theme, slug] = key.split(":") as [Theme, string];
    const bgs = Object.values(BACKGROUNDS[theme]);
    it(`${key}: text ≥ 4.5, on ≥ 4.5, on-primary ≥ 7, fill ≥ 3 (lime exception D-59)`, () => {
      for (const bg of bgs) {
        expect(contrast(s["accent-text"], bg)).toBeGreaterThanOrEqual(4.5);
        expect(contrast(s["primary-text"], bg)).toBeGreaterThanOrEqual(4.5);
      }
      expect(contrast(s["accent-on"], s["accent-fill"])).toBeGreaterThanOrEqual(4.5);
      expect(contrast(INK_LIGHT, s.primary)).toBeGreaterThanOrEqual(7);
      const fillMin = theme === "light" && slug === "sirkov" ? 2.1 : 3;
      expect(contrast(s["accent-fill"], BACKGROUNDS[theme].layer)).toBeGreaterThanOrEqual(fillMin);
    });
  }
});

describe("generator for «Свой» and seasonal themes (D-58)", () => {
  // Deterministic pseudo-random colours.
  let seed = 42;
  const rnd = () => {
    seed = (seed * 1103515245 + 12345) % 2 ** 31;
    return seed / 2 ** 31;
  };
  const hex = () =>
    `#${[0, 0, 0]
      .map(() =>
        Math.floor(rnd() * 256)
          .toString(16)
          .padStart(2, "0"),
      )
      .join("")}`;

  for (const theme of ["dark", "light"] as const) {
    it(`200 random colours meet the guarantees in ${theme} theme`, () => {
      for (let i = 0; i < 200; i++) {
        const s = generateAccentSet(hex(), theme);
        for (const bg of Object.values(BACKGROUNDS[theme])) {
          expect(contrast(s.accentFill, bg)).toBeGreaterThanOrEqual(3);
          expect(contrast(s.accentText, bg)).toBeGreaterThanOrEqual(4.5);
          expect(contrast(s.primaryText, bg)).toBeGreaterThanOrEqual(4.5);
        }
        expect(contrast(s.accentOn, s.accentFill)).toBeGreaterThanOrEqual(4.5);
        expect(contrast(s.onPrimary, s.primary)).toBeGreaterThanOrEqual(7);
      }
    });
  }
});
