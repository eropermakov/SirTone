import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ Channel: class {} }));
vi.mock("./bindings", () => ({ commands: {} }));

import { transferStage } from "./phones";

describe("transferStage", () => {
  it("reports package preparation before transfer work", () => {
    expect(transferStage("original", 0)).toBe("preparing");
    expect(transferStage("economy", 0.149)).toBe("preparing");
  });

  it("reports the reserved encoding range for compressed qualities", () => {
    expect(transferStage("economy", 0.15)).toBe("encoding");
    expect(transferStage("max", 0.59)).toBe("encoding");
  });

  it("reports sending after encoding and for original files", () => {
    expect(transferStage("economy", 0.6)).toBe("sending");
    expect(transferStage("original", 0.15)).toBe("sending");
  });
});
