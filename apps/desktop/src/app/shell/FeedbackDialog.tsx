import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  exportDiagnostics,
  type FeedbackSubmission,
  submitFeedback,
} from "@/shared/ipc/diagnostics";
import { errorCode } from "@/shared/ipc/phones";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { notify } from "@/shared/ui/notify";
import { Switch } from "@/shared/ui/Switch";

const DRAFT_KEY = "sirtone.feedback-draft.v1";
const SUPPORT_EMAIL = "sir.kov.mail@gmail.com";
const MAIL_FALLBACK_ERRORS = new Set(["NetworkOffline", "NotSupported", "RemoteError", "Internal"]);
const FIELD =
  "t-body min-h-(--button-height) rounded-(--radius-md) border border-border bg-bg-layer px-3 text-text-primary";
const KINDS = ["bug", "suggestion", "question", "other"] as const;
type Kind = FeedbackSubmission["kind"];
type Draft = Pick<FeedbackSubmission, "kind" | "message" | "email" | "includeDiagnostics">;

const readDraft = (kind: Kind): Draft => {
  try {
    const parsed = JSON.parse(localStorage.getItem(DRAFT_KEY) ?? "null") as Draft | null;
    if (parsed && ["bug", "suggestion", "question", "other"].includes(parsed.kind)) return parsed;
  } catch {
    // A stale browser draft must never prevent feedback.
  }
  return { kind, message: "", email: null, includeDiagnostics: false };
};

/** User-initiated support form. The draft remains local until the API confirms receipt. */
export function FeedbackDialog({ kind, onClose }: { kind: Kind; onClose: () => void }) {
  const { t } = useTranslation("shell");
  const [draft, setDraft] = useState<Draft>(() => readDraft(kind));
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reference, setReference] = useState<string | null>(null);

  useEffect(() => {
    localStorage.setItem(DRAFT_KEY, JSON.stringify(draft));
  }, [draft]);

  const send = () => {
    if (!draft.message.trim()) return;
    setBusy(true);
    setError(null);
    submitFeedback({ ...draft, message: draft.message.trim(), email: draft.email?.trim() || null })
      .then((receipt) => {
        if (!receipt) throw { code: "NotSupported" };
        localStorage.removeItem(DRAFT_KEY);
        setReference(receipt.referenceId);
        notify(t("feedback.sent"));
      })
      .catch((cause: unknown) => setError(errorCode(cause)))
      .finally(() => setBusy(false));
  };

  const saveReport = () => {
    setSaving(true);
    exportDiagnostics()
      .then((report) => {
        if (report) notify(t("diagnostics.saved"));
      })
      .catch(() => notify(t("diagnostics.failed")))
      .finally(() => setSaving(false));
  };

  const openMailFallback = () => {
    setSaving(true);
    const exportReport = draft.includeDiagnostics ? exportDiagnostics() : Promise.resolve(null);
    exportReport
      .then((report) => {
        const subject = `[SIRTONE] [${draft.kind.toUpperCase()}] [Windows] [${__APP_VERSION__}]${report ? ` [${report.reportId}]` : ""}`;
        const body = t("feedback.mailBody", {
          message: draft.message.trim(),
          email: draft.email?.trim() || t("feedback.mailNoEmail"),
          attachment: report
            ? t("feedback.mailAttachment", { id: report.reportId })
            : t("feedback.mailNoAttachment"),
        });
        window.location.assign(`mailto:${SUPPORT_EMAIL}?${new URLSearchParams({ subject, body })}`);
      })
      .catch(() => notify(t("diagnostics.failed")))
      .finally(() => setSaving(false));
  };

  return (
    <Dialog title={t("feedback.title")} closeLabel={t("feedback.cancel")} onClose={onClose}>
      <form
        className="flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          send();
        }}
      >
        <label className="flex flex-col gap-1">
          <span className="t-caption text-text-secondary">{t("feedback.kind")}</span>
          <select
            className={FIELD}
            value={draft.kind}
            disabled={busy || Boolean(reference)}
            onChange={(event) => setDraft((current) => ({ ...current, kind: event.target.value }))}
          >
            {KINDS.map((option) => (
              <option key={option} value={option}>
                {t(`feedback.kinds.${option}`)}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-1">
          <span className="t-caption text-text-secondary">{t("feedback.description")}</span>
          <textarea
            required
            maxLength={6000}
            rows={5}
            className={`${FIELD} py-2`}
            value={draft.message}
            disabled={busy || Boolean(reference)}
            placeholder={t(draft.kind === "bug" ? "feedback.bugHint" : "feedback.descriptionHint")}
            onChange={(event) =>
              setDraft((current) => ({ ...current, message: event.target.value }))
            }
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="t-caption text-text-secondary">{t("feedback.email")}</span>
          <input
            type="email"
            autoComplete="email"
            className={FIELD}
            value={draft.email ?? ""}
            disabled={busy || Boolean(reference)}
            placeholder={t("feedback.emailHint")}
            onChange={(event) =>
              setDraft((current) => ({ ...current, email: event.target.value || null }))
            }
          />
        </label>
        <div className="flex items-center justify-between gap-4 rounded-(--radius-md) border border-border p-3">
          <span className="flex min-w-0 flex-col gap-1">
            <span className="t-body-strong text-text-primary">{t("feedback.attachTitle")}</span>
            <span className="t-caption text-text-secondary">{t("feedback.attachHint")}</span>
          </span>
          <Switch
            checked={draft.includeDiagnostics}
            disabled={busy || Boolean(reference)}
            label={t("feedback.attachTitle")}
            onChange={(includeDiagnostics) =>
              setDraft((current) => ({ ...current, includeDiagnostics }))
            }
          />
        </div>
        {error && (
          <div className="flex flex-wrap items-center gap-2" role="alert">
            <p className="t-caption text-danger">
              {t(`feedback.errors.${error}`, { defaultValue: t("feedback.errors.Internal") })}
            </p>
            {MAIL_FALLBACK_ERRORS.has(error) && (
              <Button
                variant="secondary"
                disabled={saving || Boolean(reference)}
                onClick={openMailFallback}
              >
                {t(saving ? "diagnostics.saving" : "feedback.openMail")}
              </Button>
            )}
          </div>
        )}
        {reference && (
          <p role="status" className="t-caption text-text-secondary">
            {t("feedback.receipt", { id: reference })}
          </p>
        )}
        <div className="flex flex-wrap gap-2">
          {!reference && (
            <Button variant="primary" type="submit" disabled={busy || !draft.message.trim()}>
              {t(busy ? "feedback.sending" : "feedback.send")}
            </Button>
          )}
          <Button variant="secondary" disabled={saving} onClick={saveReport}>
            {t(saving ? "diagnostics.saving" : "feedback.saveReport")}
          </Button>
          <Button variant="ghost" onClick={onClose}>
            {t(reference ? "feedback.close" : "feedback.cancel")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
