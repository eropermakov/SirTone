import { describe, expect, it } from "vitest";
import { resetFlashBudget, tryFlash } from "./flashBudget";
import { budget, FlashClock, FX, type FxInput, fxColors, planFx } from "./fxPlan";

const base: FxInput = {
  mode: "season",
  manual: null,
  seasonEffect: "snow",
  weatherEffect: null,
  level: 3,
  milkdrop: false,
  reduced: false,
  staticWhenWeak: false,
};

describe("live effects plan (T4.20, FR-FX-001…007)", () => {
  it("follows the level budget (§13.3) and halves under MilkDrop at ≤ 3", () => {
    expect(budget(0, false).count).toBe(0);
    expect(budget(2, false)).toEqual({ count: 300, fps: 30, planes: 1 });
    expect(budget(3, false)).toEqual({ count: 1500, fps: 60, planes: 1 });
    expect(budget(4, false)).toEqual({ count: 5000, fps: 60, planes: 3 });
    expect(budget(3, true).count).toBe(750);
    expect(budget(4, true).count).toBe(5000);
  });

  it("is off at levels 0 and 1, a still frame at 1 with the switch", () => {
    expect(planFx({ ...base, level: 0 })).toBeNull();
    expect(planFx({ ...base, level: 1 })).toBeNull();
    const still = planFx({ ...base, level: 1, staticWhenWeak: true });
    expect(still?.still).toBe(true);
    expect(still?.fps).toBe(0);
  });

  it("«По сезону» is sparser; the real weather and a manual pick are not", () => {
    const season = planFx(base);
    const snow = FX.types.snow.density ?? 1;
    expect(season?.count).toBe(Math.round(1500 * snow * FX.season_density));
    const manual = planFx({ ...base, mode: "manual", manual: "snow" });
    expect(manual?.count).toBe(Math.round(1500 * snow));
    const weather = planFx({ ...base, mode: "weather", weatherEffect: "rain" });
    expect(weather?.type).toBe("rain");
    expect(weather?.count).toBe(1500);
  });

  it("without weather falls back to the season (AT-16); off is off", () => {
    expect(planFx({ ...base, mode: "weather" })?.type).toBe("snow");
    expect(planFx({ ...base, mode: "off" })).toBeNull();
    expect(planFx({ ...base, seasonEffect: null })).toBeNull();
  });

  it("reduced motion — a still frame without flashes", () => {
    const p = planFx({ ...base, mode: "manual", manual: "storm", reduced: true });
    expect(p).toMatchObject({ type: "storm", still: true, flash: false, fps: 0 });
    expect(planFx({ ...base, mode: "manual", manual: "storm" })?.flash).toBe(true);
  });

  it("noise effects have no particles", () => {
    expect(planFx({ ...base, mode: "manual", manual: "fog" })?.count).toBe(0);
  });

  it("colours come from the season palettes", () => {
    expect(fxColors("leaves")).toHaveLength(3);
    expect(fxColors("snow")).toHaveLength(1);
  });

  it("one budget for the screen: a «Волны» lift 3 s after a storm flash is refused", () => {
    resetFlashBudget();
    const clock = new FlashClock(0, () => 0);
    let first = -1;
    for (let t = 0; t < 20_000 && first < 0; t += 16) if (clock.level(t) > 0) first = t;
    expect(first).toBeGreaterThanOrEqual(10_000);
    expect(tryFlash(first + 3_000)).toBe(false);
    expect(tryFlash(first + 10_000)).toBe(true);
    // And the other way round: a lift just now delays the storm.
    resetFlashBudget();
    expect(tryFlash(50_000)).toBe(true);
    const storm = new FlashClock(40_000, () => 0);
    for (let t = 50_000; t < 59_000; t += 16) expect(storm.level(t)).toBe(0);
  });

  it("storm flashes: never two within 10 s, smooth, ≤ lift", () => {
    resetFlashBudget();
    let r = 0;
    const seq = [0, 0, 0.5, 1, 0, 0.2];
    const clock = new FlashClock(0, () => seq[r++ % seq.length] ?? 0);
    const starts: number[] = [];
    let was = 0;
    let max = 0;
    for (let t = 0; t < 120_000; t += 16) {
      const v = clock.level(t);
      max = Math.max(max, v);
      if (v > 0 && was === 0) starts.push(t);
      was = v;
    }
    expect(starts.length).toBeGreaterThan(3);
    for (let i = 1; i < starts.length; i++) {
      expect((starts[i] ?? 0) - (starts[i - 1] ?? 0)).toBeGreaterThanOrEqual(10_000);
    }
    expect(max).toBeLessThanOrEqual(FX.flash.lift);
  });
});
