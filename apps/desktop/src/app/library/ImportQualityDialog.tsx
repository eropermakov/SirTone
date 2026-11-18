import { HardDrive, Save, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ImportQuality } from "@/shared/ipc/library";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";

const OPTIONS: Array<{
  quality: ImportQuality;
  icon: typeof HardDrive;
}> = [
  { quality: "original", icon: HardDrive },
  { quality: "economy", icon: Save },
  { quality: "max", icon: Sparkles },
];

/** One explicit, batch-scoped answer before files enter the import worker (FR-IMP-011). */
export function ImportQualityDialog({
  paths,
  onChoose,
  onClose,
}: {
  paths: string[];
  onChoose: (quality: ImportQuality) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  return (
    <Dialog title={t("importQuality.title")} closeLabel={tc("errors.close")} onClose={onClose}>
      <div className="flex flex-col gap-3" data-testid="import-quality-dialog">
        <p className="t-body text-text-secondary">
          {t("importQuality.description", { count: paths.length })}
        </p>
        <div className="flex flex-col gap-2">
          {OPTIONS.map(({ quality, icon: Icon }) => (
            <Button
              key={quality}
              variant={quality === "original" ? "primary" : "secondary"}
              className={cn("h-auto min-h-(--button-height) justify-start px-4 py-3 text-left")}
              onClick={() => onChoose(quality)}
            >
              <Icon size={18} strokeWidth={1.5} />
              <span className="flex flex-col items-start gap-0.5">
                <span>{t(`importQuality.${quality}.title`)}</span>
                <span className="t-caption font-normal opacity-80">
                  {t(`importQuality.${quality}.description`)}
                </span>
              </span>
            </Button>
          ))}
        </div>
        <p className="t-caption text-text-tertiary">{t("importQuality.note")}</p>
      </div>
    </Dialog>
  );
}
