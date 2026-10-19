import { describe, expect, it } from "vitest";
import { blendLook, buildPalette, lookOf, mixOklab, motionOf, vibeColor, WAVES } from "./palette";

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
      energy: null,
      aggression: null,
      danceability: null,
      acousticness: null,
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

  it("moves a sad track slowly and an energetic one fast (T4.5, FR-VIZ-002)", () => {
    const sad = motionOf({
      arousal: 0.1,
      energy: 0.15,
      aggression: 0.05,
      danceability: 0.2,
      acousticness: 0.8,
    });
    const hot = motionOf({
      arousal: 0.9,
      energy: 0.9,
      aggression: 0.6,
      danceability: 0.85,
      acousticness: 0.1,
    });
    expect(hot.speed).toBeGreaterThan(sad.speed * 2);
    expect(hot.warpBase).toBeGreaterThan(sad.warpBase);
    expect(hot.beatGain).toBeGreaterThan(sad.beatGain);
    expect(hot.sparkleGain).toBeGreaterThan(sad.sparkleGain);
    expect(sad.grain).toBeGreaterThan(hot.grain);
    // Continuous: a small step in the vibe is a small step in the motion.
    const a = motionOf({ arousal: 0.5, energy: 0.5 });
    const b = motionOf({ arousal: 0.51, energy: 0.5 });
    expect(Math.abs(b.speed - a.speed)).toBeLessThan(0.01 * (WAVES.speed[1] - WAVES.speed[0]));
    // Not analysed: the middle.
    expect(motionOf(null).speed).toBeCloseTo((WAVES.speed[0] + WAVES.speed[1]) / 2);
  });

  it("a sad → energetic change completes within 3 s (T4.5 acceptance)", () => {
    expect(WAVES.transition_ms).toBeLessThanOrEqual(3000);
    const input = (valence: number, arousal: number) => ({
      colors: {
        valence,
        arousal,
        energy: arousal,
        aggression: 0.2,
        danceability: arousal,
        acousticness: 1 - arousal,
        cover: [],
        collection: [],
      },
      accent: "#aacc00",
      followAccent: false,
      minimal: false,
    });
    const sad = lookOf(input(0.1, 0.1));
    const hot = lookOf(input(0.9, 0.9));
    // Halfway — in between; at the end of the transition — the target exactly.
    const mid = blendLook(sad, hot, 0.5);
    expect(mid.motion.speed).toBeGreaterThan(sad.motion.speed);
    expect(mid.motion.speed).toBeLessThan(hot.motion.speed);
    const end = blendLook(sad, hot, WAVES.transition_ms / WAVES.transition_ms);
    expect(end).toEqual(hot);
    // Smooth start: barely moved after the first frame.
    const first = blendLook(sad, hot, 16 / WAVES.transition_ms);
    expect(Math.abs(first.motion.speed - sad.motion.speed)).toBeLessThan(
      0.01 * (hot.motion.speed - sad.motion.speed),
    );
  });
});
