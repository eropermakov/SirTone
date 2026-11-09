import { describe, expect, it } from "vitest";
import { ACCENT_SETS } from "../ui/tokens/tokens.generated";
import { BACKGROUNDS, contrast, generateAccentSet, type Theme } from "./color";

describe("the 10 colour sets (D-158) against the 2.1 surfaces (T9.3, Q-30)", () => {
  it("has 10 sets for each theme", () => {
    expect(Object.keys(ACCENT_SETS)).toHaveLength(10);
  });

  for (const [slug, themes] of Object.entries(ACCENT_SETS)) {
    for (const theme of ["dark", "light"] as const) {
      const s = themes[theme];
      const bgs = BACKGROUNDS[theme];
      // «Sir.kov» is the owner's own accent: on dark #650A12 is only a fill under white (DESIGN.md §4).
      const own = slug === "sirkov" && theme === "dark";
      it(`${slug}:${theme}: text ≥ 4.5, fill ≥ 3 on every surface, content on fill ≥ 4.5`, () => {
        for (const bg of bgs) {
          expect(contrast(s.accentText, bg), `text on ${bg}`).toBeGreaterThanOrEqual(4.5);
          if (!own) expect(contrast(s.accentFill, bg), `fill on ${bg}`).toBeGreaterThanOrEqual(3);
        }
        expect(contrast(s.accentOn, s.accentFill)).toBeGreaterThanOrEqual(4.5);
      });
    }
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

  for (const theme of ["dark", "light"] as Theme[]) {
    it(`200 random colours meet the guarantees in ${theme} theme`, () => {
      for (let i = 0; i < 200; i++) {
        const s = generateAccentSet(hex(), theme);
        for (const bg of BACKGROUNDS[theme]) {
          expect(contrast(s.accentFill, bg)).toBeGreaterThanOrEqual(3);
          expect(contrast(s.accentText, bg)).toBeGreaterThanOrEqual(4.5);
        }
        expect(contrast(s.accentOn, s.accentFill)).toBeGreaterThanOrEqual(4.5);
      }
    });
  }
});
