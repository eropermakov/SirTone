import { Check } from "lucide-react";
import { useTranslation } from "react-i18next";
import { type Quality, type TransferStage, transferStage } from "@/shared/ipc/phones";
import { cn } from "@/shared/lib/utils";

type TimelineStep = {
  stage: TransferStage;
  label: string;
};

/**
 * The transfer is deliberately shown as a small route, not a frozen percentage: preparation,
 * optional encoding, then cable delivery. It mirrors only real work reported by the core.
 */
export function TransferTimeline({ quality, fraction }: { quality: Quality; fraction: number }) {
  const { t } = useTranslation("shell");
  const active = transferStage(quality, fraction);
  const steps: TimelineStep[] = [
    { stage: "preparing", label: t("phones.transferring.steps.preparing") },
    ...(quality === "original"
      ? []
      : [{ stage: "encoding" as const, label: t("phones.transferring.steps.encoding") }]),
    { stage: "sending", label: t("phones.transferring.steps.sending") },
  ];
  const activeIndex = steps.findIndex((step) => step.stage === active);
  const percent = Math.round(fraction * 100);
  const activity =
    active === "preparing"
      ? t("phones.transferring.preparing")
      : active === "encoding"
        ? t("phones.transferring.encoding", { percent })
        : t("phones.transferring.progress", { percent });

  return (
    <section
      aria-label={t("phones.transferring.title")}
      className="transfer-timeline flex flex-col gap-3"
    >
      <div className="flex items-center gap-1" aria-hidden>
        {steps.map((step, index) => (
          <div
            key={step.stage}
            className={cn(
              "h-1 flex-1 rounded-full bg-bg-layer",
              index < activeIndex && "bg-accent-fill",
              index === activeIndex && "transfer-timeline-segment bg-accent-fill",
            )}
          />
        ))}
      </div>
      <ol className="flex flex-col" aria-live="polite">
        {steps.map((step, index) => {
          const done = index < activeIndex;
          const current = index === activeIndex;
          return (
            <li key={step.stage} className="grid grid-cols-[1.5rem_1fr] gap-3">
              <div className="flex flex-col items-center">
                <span
                  className={cn(
                    "grid size-6 place-items-center rounded-full border border-border bg-bg-base text-text-tertiary",
                    done && "border-accent-fill bg-accent-fill text-on-primary",
                    current && "transfer-timeline-node border-accent-fill text-accent-text",
                  )}
                  aria-hidden
                >
                  {done ? (
                    <Check className="size-3.5" strokeWidth={3} />
                  ) : (
                    <span className="size-1.5 rounded-full bg-current" />
                  )}
                </span>
                {index < steps.length - 1 && (
                  <span
                    className={cn(
                      "h-5 w-px bg-border",
                      (done || current) && "transfer-timeline-line bg-accent-fill",
                    )}
                    aria-hidden
                  />
                )}
              </div>
              <div className="min-h-11 pt-0.5">
                <p
                  className={cn(
                    "t-body text-text-secondary",
                    current && "t-body-strong text-text-primary",
                  )}
                >
                  {step.label}
                </p>
                {current && (
                  <p role="status" className="t-caption mt-1 text-text-secondary">
                    {activity}
                  </p>
                )}
              </div>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
