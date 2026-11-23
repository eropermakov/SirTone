import { isTauri } from "@tauri-apps/api/core";
import { commands, type DiagnosticStatus, type ReportExport } from "./bindings";

export type { DiagnosticStatus, ReportExport };

const unwrap = <T>(result: { status: "ok"; data: T } | { status: "error"; error: unknown }): T => {
  if (result.status === "error") throw result.error;
  return result.data;
};

/** Local-only diagnostics. This module never transmits a report. */
export const diagnosticsStatus = async (): Promise<DiagnosticStatus | null> =>
  isTauri() ? commands.diagnosticsStatus() : null;

/** Builds a ZIP under the app-local reports folder after an explicit UI action. */
export const exportDiagnostics = async (): Promise<ReportExport | null> =>
  isTauri() ? unwrap(await commands.diagnosticsExport()) : null;
