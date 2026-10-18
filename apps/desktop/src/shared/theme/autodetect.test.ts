import { describe, expect, it } from "vitest";
import { score } from "./autodetect";

const strong = { cores: 16, ramMb: 16384, discreteGpu: true };

describe("performance auto-detect (docs/06 §6.1)", () => {
  it("maps the benchmark median to a level", () => {
    expect(score(strong, 4)).toBe(4);
    expect(score(strong, 8)).toBe(3);
    expect(score(strong, 12)).toBe(2);
    expect(score(strong, 20)).toBe(1);
    expect(score(strong, 40)).toBe(0);
  });

  it("needs a discrete GPU for «Супермощное»", () => {
    expect(score({ ...strong, discreteGpu: false }, 4)).toBe(3);
  });

  it("caps by memory and cores", () => {
    expect(score({ cores: 2, ramMb: 16384, discreteGpu: true }, 4)).toBe(1);
    expect(score({ cores: 8, ramMb: 3000, discreteGpu: true }, 4)).toBe(1);
    expect(score({ cores: 4, ramMb: 16384, discreteGpu: true }, 4)).toBe(2);
  });

  it("falls back to hardware without WebGL", () => {
    expect(score(strong, null)).toBe(2);
    expect(score({ cores: 4, ramMb: 4096, discreteGpu: false }, null)).toBe(1);
  });
});
