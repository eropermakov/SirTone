import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";

export type ImportQualityKey = "original" | "high" | "standard" | "compact";

const OPTIONS: ImportQualityKey[] = ["original", "high", "standard", "compact"];

/** A per-import choice. The selected policy is persisted before files enter the import queue. */
export function ImportQualityDialog({
  initial,
  onChoose,
  onClose,
}: {
  initial: ImportQualityKey;
  onChoose: (quality: ImportQualityKey) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [quality, setQuality] = useState<ImportQualityKey>(initial);
  return (
    <Dialog title={t("importQuality.chooseTitle")} closeLabel={tc("errors.close")} onClose={onClose}>
      <div className="flex flex-col gap-4">
        <p className="t-body text-text-secondary">{t("importQuality.chooseHint")}</p>
        <div className="grid gap-2">
          {OPTIONS.map((key) => (
            <button
              key={key}
              type="button"
              onClick={() => setQuality(key)}
              className={`micro-press flex min-h-(--button-height) items-center rounded-(--radius-md) border px-3 text-left t-body ${quality === key ? "border-accent bg-bg-layer text-text-primary" : "border-border text-text-secondary"}`}
              aria-pressed={quality === key}
            >
              {t(`importQuality.${key}`)}
            </button>
          ))}
        </div>
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>{tc("actions.cancel")}</Button>
          <Button onClick={() => onChoose(quality)}>{t("importQuality.start")}</Button>
        </div>
      </div>
    </Dialog>
  );
}
