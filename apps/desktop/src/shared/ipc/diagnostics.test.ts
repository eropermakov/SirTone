import { beforeEach, describe, expect, it, vi } from "vitest";

const { isTauri, diagnosticsRecordFrontend } = vi.hoisted(() => ({
  isTauri: vi.fn(),
  diagnosticsRecordFrontend: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ isTauri }));
vi.mock("./bindings", () => ({
  commands: { diagnosticsRecordFrontend },
}));

import { recordFrontendDiagnostic } from "./diagnostics";

describe("recordFrontendDiagnostic", () => {
  beforeEach(() => {
    isTauri.mockReturnValue(true);
    diagnosticsRecordFrontend.mockReset().mockResolvedValue(undefined);
  });

  it("forwards only the fixed frontend failure category", async () => {
    recordFrontendDiagnostic("js_unhandled_exception");
    await Promise.resolve();
    expect(diagnosticsRecordFrontend).toHaveBeenCalledWith("js_unhandled_exception");
  });

  it("does not create an unhandled rejection when diagnostics itself is unavailable", async () => {
    diagnosticsRecordFrontend.mockRejectedValueOnce(new Error("offline"));
    recordFrontendDiagnostic("promise_rejection");
    await Promise.resolve();
    await Promise.resolve();
    expect(diagnosticsRecordFrontend).toHaveBeenCalledWith("promise_rejection");
  });
});
