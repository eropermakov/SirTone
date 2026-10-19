import { describe, expect, it } from "vitest";
import { buildPalette, mixOklab, vibeColor, WAVES } from "./palette";

describe("waves palette (T4.2, 06 §7.2)", () => {
  it("puts the vibe corners on their hues", () => {
    expect(vibeColor(0, 0).h).toBeCloseTo(WAVES.hue.sad, 0);
    expect(vibeColor(0, 1).h).toBeCloseTo(WAVES.hue.angry, 0);
    expect(vibeColor(1, 1).h).toBeCloseTo(WAVES.hue.joy, 0);
    expect(vibeColor(1, 0).h).toBeCloseTo(WAVES.hue.calm, 0);
  });

  it("scales chroma by arousal and lightness by valence", () => {
    expect(vibeColor(0.5, 0).c).toBeCloseTo(WAVES.chroma[0]);
    expect(vibeColor(0.5, 1).c).toBeCloseTo(WAVES.chroma[1]);
    expect(vibeColor(0, 0.5).l).toBeCloseTo(WAVES.lightness[0]);
    expect(vibeColor(1, 0.5).l).toBeCloseTo(WAVES.lightness[1]);
  });

  it("mixes in OKLab", () => {
    const a = { l: 0.5, c: 0.1, h: 0 };
    const b = { l: 0.7, c: 0.1, h: 180 };
    const m = mixOklab(a, b, 0.5);
    expect(m.l).toBeCloseTo(0.6);
    expect(m.c).toBeCloseTo(0, 5);
    expect(mixOklab(a, b, 0).h).toBeCloseTo(0);
  });

  it("builds four colours, or two for the minimal screen", () => {
    const colors = {
      valence: 0.2,
      arousal: 0.3,
      cover: [{ l: 0.6, c: 0.15, h: 30, w: 0.6 }],
      collection: [{ l: 0.5, c: 0.1, h: 200, w: 1 }],
    };
    const full = buildPalette({ colors, accent: "#aacc00", followAccent: true, minimal: false });
    expect(full).toHaveLength(4);
    for (const c of full) for (const v of c) expect(v).toBeGreaterThanOrEqual(0);
    const min = buildPalette({ colors, accent: "#aacc00", followAccent: true, minimal: true });
    expect(new Set(min.map((c) => c.join())).size).toBe(2);
    // Without the accent toggle the accent does not move the colour.
    const off = buildPalette({ colors, accent: "#aacc00", followAccent: false, minimal: false });
    const off2 = buildPalette({ colors, accent: "#0000ff", followAccent: false, minimal: false });
    expect(off[0]).toEqual(off2[0]);
    // Nothing known: still a palette (the accent).
    expect(
      buildPalette({ colors: null, accent: "#aacc00", followAccent: true, minimal: false }),
    ).toHaveLength(4);
  });
});
