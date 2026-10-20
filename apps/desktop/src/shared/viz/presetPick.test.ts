import { describe, expect, it } from "vitest";
import { distance, MILKDROP, type PresetDesc, pick, rank, targetOf } from "./presetPick";

const p = (name: string, hue: number, motion: number, bright: number): PresetDesc => ({
  name,
  author: "t",
  hue,
  sat: 0.6,
  bright,
  motion,
  react: motion,
  chaos: 0,
});

const index = [
  p("calm-teal", 170, 0.01, 0.2),
  p("sad-indigo", 265, 0.005, 0.08),
  p("hot-red", 20, 0.08, 0.3),
  p("joy-amber", 50, 0.06, 0.4),
];

describe("MilkDrop preset pick (T4.3, 06 §7.3)", () => {
  const ranked = rank(index);
  const sad = targetOf({
    valence: 0.05,
    arousal: 0.05,
    energy: 0.1,
    aggression: 0,
    danceability: 0.1,
    acousticness: 0.8,
    cover: [],
    collection: [],
  });
  const hot = targetOf({
    valence: 0.1,
    arousal: 0.95,
    energy: 0.95,
    aggression: 0.9,
    danceability: 0.9,
    acousticness: 0.1,
    cover: [],
    collection: [],
  });

  it("puts a sad track near the slow indigo preset, an angry one near the fast red", () => {
    const best = (t: typeof sad) =>
      [...ranked].sort((a, b) => distance(a, t) - distance(b, t))[0]?.name;
    expect(best(sad)).toBe("sad-indigo");
    expect(best(hot)).toBe("hot-red");
  });

  it("skips the recent ones, drops ✕ and prefers ♥", () => {
    const one = { ...MILKDROP, top_k: 1 };
    expect(pick(ranked, sad, { recent: [], ratings: new Map() }, one)).toBe("sad-indigo");
    expect(pick(ranked, sad, { recent: ["sad-indigo"], ratings: new Map() }, one)).not.toBe(
      "sad-indigo",
    );
    expect(pick(ranked, sad, { recent: [], ratings: new Map([["sad-indigo", -1]]) }, one)).not.toBe(
      "sad-indigo",
    );
    const liked = pick(ranked, hot, { recent: [], ratings: new Map([["joy-amber", 1]]) }, one);
    expect(["hot-red", "joy-amber"]).toContain(liked);
    // Everything recent: still something.
    expect(pick(ranked, sad, { recent: index.map((x) => x.name), ratings: new Map() })).not.toBe(
      null,
    );
  });

  it("«Только мои»: only the ♥ ones, even off the vibe and recently shown; none liked — all", () => {
    const liked = new Map([["hot-red", 1]]);
    for (let i = 0; i < 20; i++) {
      expect(pick(ranked, sad, { recent: ["hot-red"], ratings: liked, onlyLiked: true })).toBe(
        "hot-red",
      );
    }
    expect(pick(ranked, sad, { recent: [], ratings: new Map(), onlyLiked: true })).not.toBe(null);
  });
});
