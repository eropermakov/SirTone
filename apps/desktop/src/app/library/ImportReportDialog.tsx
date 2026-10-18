import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { type ImportReport, importReport } from "@/shared/ipc/library";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";

/**
 * The import report in full (T3.21, FR-IMP-006, FR-DUP-002; `06` «итоговый отчёт»): four
 * sections — added · to «Неизвестные» · already in the library (exact copies) · not added with
 * the reason. Duplicates by sound are found later, after the analysis — the «Дубли» button.
 */
export function ImportReportDialog({
  batch,
  onClose,
  onUnknown,
  onDuplicates,
}: {
  batch: number;
  onClose: () => void;
  onUnknown: () => void;
  onDuplicates: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [r, setR] = useState<ImportReport | null>(null);
  useEffect(() => {
    importReport(batch)
      .then(setR)
      .catch(() => setR({ added: [], unknown: [], duplicates: [], rejected: [] }));
  }, [batch]);
  const section = (title: string, rows: string[]) =>
    rows.length > 0 && (
      <section className="flex flex-col gap-1">
        <h3 className="t-body-strong text-text-primary">{title}</h3>
        <ul className="flex max-h-48 flex-col gap-0.5 overflow-auto">
          {rows.slice(0, 300).map((row, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: rows may repeat (same file twice)
            <li key={i} className="t-caption truncate text-text-secondary">
              {row}
            </li>
          ))}
        </ul>
      </section>
    );
  return (
    <Dialog title={t("importReport.title")} closeLabel={tc("errors.close")} onClose={onClose} wide>
      {r && (
        <>
          {section(
            t("importReport.added", { count: r.added.length }),
            r.added.map((x) => `${x.title} — ${x.artists}`),
          )}
          {r.unknown.length > 0 && (
            <div className="flex flex-col gap-2">
              {section(
                t("importReport.unknown", { count: r.unknown.length }),
                r.unknown.map((x) => x.title),
              )}
              <Button variant="ghost" className="self-start" onClick={onUnknown}>
                {t("importReport.openUnknown")}
              </Button>
            </div>
          )}
          {section(t("importReport.duplicates", { count: r.duplicates.length }), r.duplicates)}
          {section(
            t("importReport.rejected", { count: r.rejected.length }),
            r.rejected.map((x) => `${x.path} — ${t(`import.reason.${x.reason}`)}`),
          )}
          <div className="flex flex-wrap items-center gap-3">
            <span className="t-caption flex-1 text-text-secondary">{t("importReport.sound")}</span>
            <Button variant="ghost" onClick={onDuplicates}>
              {t("dups.title")}
            </Button>
          </div>
        </>
      )}
    </Dialog>
  );
}
