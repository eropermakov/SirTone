import { isTauri } from "@tauri-apps/api/core";
import {
  commands,
  type DiagnosticStatus,
  type FeedbackReceipt,
  type FeedbackSubmission,
  type ReportExport,
} from "./bindings";

export type { DiagnosticStatus, FeedbackReceipt, FeedbackSubmission, ReportExport };

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

/** Explicit privacy control; it clears only diagnostics and creates a new random installation ID. */
export const resetDiagnostics = async (): Promise<DiagnosticStatus | null> =>
  isTauri() ? commands.diagnosticsReset() : null;

/** Explicit, consented support request. No report leaves the device unless `includeDiagnostics` is on. */
export const submitFeedback = async (
  submission: FeedbackSubmission,
): Promise<FeedbackReceipt | null> =>
  isTauri() ? unwrap(await commands.feedbackSubmit(submission)) : null;
