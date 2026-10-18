import { describe, expect, it } from "vitest";
import { accelFromEvent } from "./hotkeys";

const ev = (
  code: string,
  mods: Partial<Record<"ctrl" | "alt" | "shift" | "meta", boolean>> = {},
) => ({
  key: "",
  code,
  ctrlKey: !!mods.ctrl,
  altKey: !!mods.alt,
  shiftKey: !!mods.shift,
  metaKey: !!mods.meta,
});

describe("accelFromEvent", () => {
  it("builds accelerators in the settings format", () => {
    expect(accelFromEvent(ev("ArrowRight", { ctrl: true, alt: true }))).toBe("Ctrl+Alt+Right");
    expect(accelFromEvent(ev("KeyP", { ctrl: true, shift: true }))).toBe("Ctrl+Shift+P");
    expect(accelFromEvent(ev("F9", { alt: true }))).toBe("Alt+F9");
    expect(accelFromEvent(ev("Digit5", { meta: true }))).toBe("Super+5");
  });

  it("is layout independent (physical keys)", () => {
    // Russian layout: the P key types «з», its code is still KeyP.
    expect(accelFromEvent({ ...ev("KeyP", { ctrl: true }), key: "з" })).toBe("Ctrl+P");
  });

  it("rejects bare modifiers and combinations that would steal typing", () => {
    expect(accelFromEvent(ev("ControlLeft", { ctrl: true }))).toBeNull();
    expect(accelFromEvent(ev("KeyA"))).toBeNull();
    expect(accelFromEvent(ev("KeyA", { shift: true }))).toBeNull();
  });
});
