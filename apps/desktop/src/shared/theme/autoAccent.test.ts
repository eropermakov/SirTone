import { describe, expect, it } from "vitest";
import { pickSlug } from "./autoPick";

// Light-theme fills (sirtone.css): enough for the nearest-set step.
const FILLS: Record<string, string> = {
  sirkov: "#66bb00",
  blue: "#2c68e7",
  violet: "#6a3df0",
  lilac: "#8a4fd6",
  lavender: "#a86ccf",
  gray: "#6b6b6b",
  black: "#1c1616",
  yellow: "#ad8103",
  orange: "#d76b09",
  ginger: "#bf4d09",
  coral: "#e5533d",
  brick: "#b0482c",
  lightred: "#f05252",
  red: "#dc1e20",
  darkred: "#a8121e",
  burgundy: "#8f1d4a",
  bordeaux: "#7a1c1c",
};

/** A 64×64 image filled with `rgb`, with an optional second colour on part of it. */
function image(
  rgb: [number, number, number],
  other?: { rgb: [number, number, number]; share: number },
) {
  const px = new Uint8ClampedArray(64 * 64 * 4);
  const cut = other ? Math.floor(64 * 64 * other.share) : 0;
  for (let i = 0; i < 64 * 64; i++) {
    const c = i < cut && other ? other.rgb : rgb;
    px.set([c[0], c[1], c[2], 255], i * 4);
  }
  return px;
}

describe("«Авто» from the cover (DESIGN.md §4)", () => {
  it("a blue cover gives the blue set, a green one the Sir.kov lime", () => {
    expect(pickSlug(image([40, 100, 230]), FILLS)).toBe("blue");
    expect(pickSlug(image([90, 180, 20]), FILLS)).toBe("sirkov");
  });

  it("greys: grey, or black when dark", () => {
    expect(pickSlug(image([128, 128, 128]), FILLS)).toBe("gray");
    expect(pickSlug(image([30, 30, 32]), FILLS)).toBe("black");
  });

  it("ignores near-black and near-white pixels", () => {
    // Mostly black with a red stripe: the red wins.
    expect(pickSlug(image([2, 2, 2], { rgb: [220, 30, 32], share: 0.2 }), FILLS)).toBe("red");
    expect(pickSlug(image([255, 255, 255]), FILLS)).toBe("sirkov");
  });

  it("chroma outweighs area: a saturated orange beats a larger grey", () => {
    expect(pickSlug(image([120, 120, 120], { rgb: [215, 107, 9], share: 0.4 }), FILLS)).toBe(
      "orange",
    );
  });
});
