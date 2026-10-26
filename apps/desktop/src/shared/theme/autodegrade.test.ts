import { describe, expect, it } from "vitest";
import { FrameWatch } from "./autodegrade";

/** Frames every `frameMs` from `from` for `ms`, checking after each; the times it said degrade. */
function run(w: FrameWatch, from: number, ms: number, frameMs: number): number[] {
  const hits: number[] = [];
  for (let t = from; t < from + ms; t += frameMs) {
    w.frame(t);
    if (w.check(t)) hits.push(t);
  }
  return hits;
}

describe("auto-degrade D-47 (T4.16)", () => {
  it("smooth 60 FPS never degrades", () => {
    const w = new FrameWatch();
    w.reset(0);
    expect(run(w, 0, 20_000, 16.7)).toEqual([]);
  });

  it("30 ms frames for 3 s degrade once, after the 2 s grace and 3 s of sustain", () => {
    const w = new FrameWatch();
    w.reset(0);
    const hits = run(w, 0, 6_000, 30);
    expect(hits.length).toBe(1);
    // Grace (2 s) + the first window (1 s) + sustain (3 s) ≈ 5–6 s.
    expect(hits[0]).toBeGreaterThan(4_900);
    // Still slow: the next one not before 10 s.
    const more = run(w, 6_000, 8_000, 30);
    expect(more).toEqual([]);
    expect(run(w, 14_000, 3_000, 30).length).toBe(1);
  });

  it("a 2 s hiccup does not degrade; neither does a hidden window", () => {
    const w = new FrameWatch();
    w.reset(0);
    run(w, 0, 3_000, 16.7);
    expect(run(w, 3_000, 2_000, 35)).toEqual([]);
    expect(run(w, 5_000, 3_000, 16.7)).toEqual([]);
    // A hidden window: frames 1 s apart are pauses, not slow frames.
    expect(run(w, 8_000, 10_000, 1_000)).toEqual([]);
  });

  it("a very weak machine: ≈ 380 ms frames are slow frames, not pauses (T7.1, D-118)", () => {
    const w = new FrameWatch();
    w.reset(0);
    const hits = run(w, 0, 8_000, 380);
    expect(hits.length).toBe(1);
    expect(hits[0]).toBeGreaterThan(4_900);
  });

  it("one 600 ms stall at 60 FPS does not degrade", () => {
    const w = new FrameWatch();
    w.reset(0);
    run(w, 0, 3_000, 16.7);
    w.frame(3_600);
    w.check(3_600);
    expect(run(w, 3_616, 6_000, 16.7)).toEqual([]);
  });

  it("a 1.5 s gap is a pause: the sustain starts again", () => {
    const w = new FrameWatch();
    w.reset(0);
    run(w, 0, 4_500, 30); // slow for 2.5 s past the grace — almost
    expect(run(w, 6_000, 2_000, 30)).toEqual([]); // the gap reset it
  });

  it("a screen change restarts the grace", () => {
    const w = new FrameWatch();
    w.reset(0);
    run(w, 0, 4_500, 30); // almost
    w.reset(4_500);
    expect(run(w, 4_500, 2_500, 30)).toEqual([]);
  });
});
