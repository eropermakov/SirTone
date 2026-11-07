import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { BACKGROUNDS, contrast, generateAccentSet, ON_PRIMARY, type Theme } from "./color";

const tokensCss = readFileSync(new URL("../ui/tokens/sirtone.css", import.meta.url), "utf8");
const extraCss = readFileSync(new URL("../ui/tokens/extra.css", import.meta.url), "utf8");

type Set = Record<"accent-fill" | "accent-text" | "accent-on" | "primary" | "primary-text", string>;

const hexVars = (block: string) =>
  Object.fromEntries(
    [...block.matchAll(/--([a-z-]+):\s*(#[0-9a-f]{6})/g)].map((v) => [v[1], v[2]]),
  ) as Record<string, string>;

/** Parses the 17 owner sets × 2 themes from DESIGN.md §13 CSS (copied verbatim into sirtone.css). */
function parseSets(): Map<string, Set> {
  const sets = new Map<string, Set>();
  const re = /\[data-theme="(dark|light)"\]\[data-accent="([a-z]+)"\][^{]*\{([^}]*)\}/g;
  for (const m of tokensCss.matchAll(re)) sets.set(`${m[1]}:${m[2]}`, hexVars(m[3] ?? "") as Set);
  return sets;
}

/** The neutrals of one theme from sirtone.css (DESIGN.md §3). */
function neutrals(theme: Theme): Record<string, string> {
  const sel =
    theme === "dark"
      ? ':root,\n[data-theme="dark"] {\n  --bg-base'
      : '[data-theme="light"] {\n  --bg-base';
  const i = tokensCss.indexOf(sel);
  return hexVars(tokensCss.slice(i, tokensCss.indexOf("}", i)));
}

describe("DESIGN.md colour sets, palette 2.0 (T0.9, T7.7, D-144)", () => {
  const sets = parseSets();

  it("has 17 sets for each theme", () => {
    expect(sets.size).toBe(34);
  });

  it("the shared backgrounds and on-primary match the CSS neutrals", () => {
    for (const theme of ["dark", "light"] as const) {
      const n = neutrals(theme);
      expect(BACKGROUNDS[theme]).toEqual({
        base: n["bg-base"],
        layer: n["bg-layer"],
        flyout: n["bg-flyout"],
      });
      expect(ON_PRIMARY[theme]).toBe(n["on-primary"]);
    }
  });

  for (const [key, s] of parseSets()) {
    const theme = key.split(":")[0] as Theme;
    const bgs = Object.values(BACKGROUNDS[theme]);
    it(`${key}: text ≥ 4.5, fill ≥ 3 on every background, on ≥ 4.5, on-primary ≥ 7 — no exceptions`, () => {
      for (const bg of bgs) {
        expect(contrast(s["accent-text"], bg), `text on ${bg}`).toBeGreaterThanOrEqual(4.5);
        expect(contrast(s["primary-text"], bg), `primary-text on ${bg}`).toBeGreaterThanOrEqual(
          4.5,
        );
        expect(contrast(s["accent-fill"], bg), `fill on ${bg}`).toBeGreaterThanOrEqual(3);
      }
      expect(contrast(s["accent-on"], s["accent-fill"])).toBeGreaterThanOrEqual(4.5);
      expect(contrast(ON_PRIMARY[theme], s.primary)).toBeGreaterThanOrEqual(7);
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

describe("neutrals and owner palettes (D-144, D-74)", () => {
  const block = (sel: string) => {
    const i = extraCss.indexOf(sel);
    return hexVars(extraCss.slice(i, extraCss.indexOf("}", i)));
  };

  for (const theme of ["dark", "light"] as const) {
    const n = neutrals(theme);
    const surfaces = ["bg-base", "bg-layer", "bg-raised", "bg-flyout"].map((k) => n[k] as string);

    it(`${theme}: the surface ladder only gets lighter from the window up`, () => {
      const lums = surfaces.map((h) => contrast(h, "#000000"));
      expect([...lums].sort((a, b) => a - b)).toEqual(lums);
    });

    it(`${theme}: text, icons and danger ≥ 4.5:1 on every surface; inactive is not text`, () => {
      for (const k of ["text-primary", "text-secondary", "text-tertiary", "icon", "danger"]) {
        for (const bg of surfaces) {
          expect(contrast(n[k] as string, bg), `${k} on ${bg}`).toBeGreaterThanOrEqual(4.5);
        }
      }
      // The owner's «inactive» is a track/fill tone (1.25:1 dark, 1.4:1 light), never text.
      expect(contrast(n.inactive as string, surfaces[0] as string)).toBeGreaterThanOrEqual(1.2);
    });
  }

  // «Оранжевая» and «Чёрно-белая» inherit the dark neutrals; their own colours are not changed.
  const dark = neutrals("dark");
  const darkSurfaces = ["bg-base", "bg-layer", "bg-raised", "bg-flyout"].map(
    (k) => dark[k] as string,
  );
  for (const p of ["ember", "mono"]) {
    it(`palette ${p}: text ≥ 4.5, fill ≥ 3 on the dark surfaces, on ≥ 4.5, on-primary ≥ 7`, () => {
      const v = block(`:root[data-theme="dark"][data-palette="${p}"]`);
      for (const bg of darkSurfaces) {
        expect(contrast(v["accent-text"] as string, bg)).toBeGreaterThanOrEqual(4.5);
        expect(contrast(v["primary-text"] as string, bg)).toBeGreaterThanOrEqual(4.5);
        expect(contrast(v["accent-fill"] as string, bg)).toBeGreaterThanOrEqual(3);
      }
      expect(contrast(v["accent-on"] as string, v["accent-fill"] as string)).toBeGreaterThanOrEqual(
        4.5,
      );
      expect(contrast(v["on-primary"] as string, v.primary as string)).toBeGreaterThanOrEqual(7);
    });
  }
});
