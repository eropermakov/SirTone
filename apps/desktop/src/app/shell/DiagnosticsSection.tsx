import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  type DiagnosticStatus,
  diagnosticsStatus,
  exportDiagnostics,
  type ReportExport,
} from "@/shared/ipc/diagnostics";
import { Button } from "@/shared/ui/Button";
import { notify } from "@/shared/ui/notify";
import { FeedbackDialog } from "./FeedbackDialog";

/** Local report export and explicit feedback. No data is sent until the user confirms the form. */
export function DiagnosticsSection() {
  const { t } = useTranslation("shell");
  const [status, setStatus] = useState<DiagnosticStatus | null>(null);
  const [report, setReport] = useState<ReportExport | null>(null);
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<"bug" | "suggestion" | "question" | "other" | null>(
    null,
  );

  useEffect(() => {
    void diagnosticsStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  const save = () => {
    setBusy(true);
    exportDiagnostics()
      .then((next) => {
        if (!next) return;
        setReport(next);
        notify(t("diagnostics.saved"));
      })
      .catch(() => notify(t("diagnostics.failed")))
      .finally(() => setBusy(false));
  };

  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-body-strong text-text-primary">{t("diagnostics.title")}</h2>
      <div className="flex flex-col gap-3 rounded-(--radius-md) border border-border bg-bg-raised p-6">
        <p className="t-body text-text-primary">{t("diagnostics.description")}</p>
        <p className="t-caption text-text-secondary">{t("diagnostics.contents")}</p>
        {status?.previousUncleanShutdown && (
          <p className="t-caption text-text-secondary" role="status">
            {t("diagnostics.previousUnclean")}
          </p>
        )}
        <div className="flex flex-wrap gap-3">
          <Button variant="primary" onClick={() => setFeedback("bug")}>
            {t("feedback.reportBug")}
          </Button>
          <Button variant="secondary" onClick={() => setFeedback("suggestion")}>
            {t("feedback.suggest")}
          </Button>
          <Button variant="secondary" disabled={busy} onClick={save}>
            {t(busy ? "diagnostics.saving" : "diagnostics.save")}
          </Button>
        </div>
        {report && (
          <p className="t-caption break-all text-text-secondary" role="status">
            {t("diagnostics.savedAt", { path: report.path })}
          </p>
        )}
      </div>
      {feedback && <FeedbackDialog kind={feedback} onClose={() => setFeedback(null)} />}
    </div>
  );
}
