import { describe, expect, it } from "vitest";
import { profile } from "./performance";

describe("performance presets (docs/06 §6)", () => {
  it("maps levels to the table", () => {
    expect(profile(0)).toEqual({
      motionScale: 0,
      uiAnimations: false,
      blur: false,
      listAnimations: false,
    });
    expect(profile(1).motionScale).toBe(0.5);
    expect(profile(2)).toMatchObject({ motionScale: 1, blur: true, listAnimations: true });
    expect(profile(4).motionScale).toBe(1.5);
  });

  it("point switches win over the level", () => {
    expect(profile(4, { ui_animations: false }).motionScale).toBe(0);
    expect(profile(0, { ui_animations: true }).motionScale).toBe(1);
    expect(profile(3, { blur: false }).blur).toBe(false);
  });

  it("clamps nonsense", () => {
    expect(profile(9).motionScale).toBe(1.5);
    expect(profile(-3).motionScale).toBe(0);
  });
});
