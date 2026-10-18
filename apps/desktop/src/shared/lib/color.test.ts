import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { BACKGROUNDS, contrast, generateAccentSet, INK_LIGHT, type Theme } from "./color";

const tokensCss = readFileSync(new URL("../ui/tokens/sirtone.css", import.meta.url), "utf8");
const extraCss = readFileSync(new URL("../ui/tokens/extra.css", import.meta.url), "utf8");

type Set = Record<"accent-fill" | "accent-text" | "accent-on" | "primary" | "primary-text", string>;

const hexVars = (block: string) =>
  Object.fromEntries(
    [...block.matchAll(/--([a-z-]+):\s*(#[0-9a-f]{6})/g)].map((v) => [v[1], v[2]]),
  ) as Record<string, string>;

/**
 * Parses the 17 owner sets × 2 themes from DESIGN.md §13 CSS (copied verbatim into sirtone.css),
 * then applies the dark lifts from extra.css (D-75: the #333333 ladder needs lighter accent text).
 */
function parseSets(): Map<string, Set> {
  const sets = new Map<string, Set>();
  const re = /\[data-theme="(dark|light)"\]\[data-accent="([a-z]+)"\][^{]*\{([^}]*)\}/g;
  for (const m of tokensCss.matchAll(re)) sets.set(`${m[1]}:${m[2]}`, hexVars(m[3] ?? "") as Set);
  for (const m of extraCss.matchAll(re)) {
    const key = `${m[1]}:${m[2]}`;
    sets.set(key, { ...sets.get(key), ...hexVars(m[3] ?? "") } as Set);
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

describe("dark neutrals and owner palettes on #333333 (D-75)", () => {
  const block = (sel: string) => {
    const i = extraCss.indexOf(sel);
    return hexVars(extraCss.slice(i, extraCss.indexOf("}", i)));
  };
  const neutrals = block(':root:not([data-theme="light"]) {');
  const surfaces = ["bg-base", "bg-layer", "bg-raised", "bg-flyout"].map(
    (k) => neutrals[k] as string,
  );

  it("base is the owner's #333333 and the ladder only gets lighter", () => {
    expect(neutrals["bg-base"]).toBe("#333333");
    const lums = surfaces.map((h) => contrast(h, "#000000"));
    expect([...lums].sort((a, b) => a - b)).toEqual(lums);
  });

  it("text, icons and danger are ≥ 4.5:1 on every surface; inactive is visible (≥ 1.8)", () => {
    for (const k of ["text-primary", "text-secondary", "text-tertiary", "icon", "danger"]) {
      for (const bg of surfaces) {
        expect(contrast(neutrals[k] as string, bg), `${k} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      }
    }
    expect(contrast(neutrals.inactive as string, surfaces[0] as string)).toBeGreaterThanOrEqual(
      1.8,
    );
  });

  for (const p of ["ember", "mono"]) {
    it(`palette ${p}: accent text ≥ 4.5 on surfaces, on ≥ 4.5 to fill, on-primary ≥ 7`, () => {
      const v = block(`:root[data-theme="dark"][data-palette="${p}"]`);
      for (const bg of surfaces) {
        expect(contrast(v["accent-text"] as string, bg)).toBeGreaterThanOrEqual(4.5);
        expect(contrast(v["primary-text"] as string, bg)).toBeGreaterThanOrEqual(4.5);
      }
      expect(contrast(v["accent-on"] as string, v["accent-fill"] as string)).toBeGreaterThanOrEqual(
        4.5,
      );
      expect(contrast(v["on-primary"] as string, v.primary as string)).toBeGreaterThanOrEqual(7);
    });
  }
});
