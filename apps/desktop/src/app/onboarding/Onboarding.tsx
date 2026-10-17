import { FolderDown, Heart, MousePointerClick, Search } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { RootPanel } from "@/app/library/RootPanel";
import {
  AccentSwatches,
  PerformanceSection,
  SeasonSwitch,
  ThemeChips,
} from "@/app/shell/SettingsScreen";
import type { LibraryStatus } from "@/shared/ipc/library";
import { patchSettings } from "@/shared/ipc/settings";
import type { AccentSource, ThemePref } from "@/shared/theme/theme";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Dialog } from "@/shared/ui/Dialog";

const STEPS = 4;

/**
 * First-start setup (T2.15, FR-ONB-001/003; DESIGN.md §1 Zeigarnik, §9 neutral Panel «1 из 4»):
 * one step at a time — theme and language; colour and «Тема по сезону»; performance (manual or
 * «Определить автоматически»); the music folder. The same controls as Settings, applied at
 * once. No taste questionnaire (TZ). «Пропустить» leaves the rest to Settings.
 */
export function Onboarding({
  theme,
  accent,
  onTheme,
  onAccent,
  status,
  onRoot,
  onFinish,
}: {
  theme: ThemePref;
  accent: AccentSource;
  onTheme: (t: ThemePref) => void;
  onAccent: (a: AccentSource) => void;
  status: LibraryStatus;
  onRoot: (s: LibraryStatus) => void;
  onFinish: () => void;
}) {
  const { t } = useTranslation("shell");
  const [step, setStep] = useState(0);
  const finish = () => {
    patchSettings({ onboarding: { done: true } }).catch(() => {});
    onFinish();
  };
  const titles = [
    "onboarding.theme",
    "onboarding.color",
    "onboarding.performance",
    "onboarding.folder",
  ];
  return (
    <div className="fixed inset-0 z-(--z-modal) flex items-center justify-center bg-bg-base">
      <section
        role="dialog"
        aria-modal="true"
        aria-label={t("onboarding.title")}
        className="flyout-in flex max-h-[92vh] w-[640px] max-w-[94vw] flex-col gap-5 overflow-auto rounded-(--radius-xl) border border-border bg-bg-raised p-6"
      >
        <div className="flex items-baseline justify-between gap-4">
          <h1 className="t-card-title text-text-primary">{t("onboarding.title")}</h1>
          <span className="t-caption text-text-tertiary">
            {t("onboarding.step", { n: step + 1, total: STEPS })}
          </span>
        </div>
        <div className="h-2 overflow-hidden rounded-full bg-inactive">
          <div
            className="h-full origin-left rounded-full bg-accent-text transition-transform duration-(--dur-normal)"
            style={{ transform: `scaleX(${(step + 1) / STEPS})` }}
          />
        </div>
        <h2 className="t-shelf-title text-primary-text">{t(titles[step] ?? "")}</h2>
        {step === 0 && (
          <div className="flex flex-col gap-4">
            <ThemeChips theme={theme} onTheme={onTheme} />
            <div className="flex flex-col gap-2">
              <span className="t-body-strong text-text-primary">{t("onboarding.language")}</span>
              <div className="flex gap-2">
                <Chip active onClick={() => {}}>
                  {t("onboarding.russian")}
                </Chip>
              </div>
            </div>
          </div>
        )}
        {step === 1 && (
          <div className="flex flex-col gap-4">
            <AccentSwatches theme={theme} accent={accent} onAccent={onAccent} />
            <SeasonSwitch />
          </div>
        )}
        {step === 2 && <PerformanceSection />}
        {step === 3 && (
          <RootPanel
            bare
            status={status}
            onDone={(s) => {
              onRoot(s);
              finish();
            }}
          />
        )}
        <div className="flex items-center gap-3 border-t border-separator pt-4">
          <Button variant="ghost" onClick={finish}>
            {t("onboarding.skip")}
          </Button>
          <div className="flex-1" />
          {step > 0 && (
            <Button variant="secondary" onClick={() => setStep((s) => s - 1)}>
              {t("onboarding.back")}
            </Button>
          )}
          {step < STEPS - 1 && (
            <Button variant="primary" onClick={() => setStep((s) => s + 1)}>
              {t("onboarding.next")}
            </Button>
          )}
        </div>
      </section>
    </div>
  );
}

const TIPS = [
  { icon: FolderDown, key: "drop" },
  { icon: MousePointerClick, key: "menu" },
  { icon: Search, key: "search" },
  { icon: Heart, key: "wave" },
] as const;

/** The tips (FR-ONB-002): how to bring music in and the main moves; again from Settings. */
export function Tips({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const close = () => {
    patchSettings({ onboarding: { tips_seen: true } }).catch(() => {});
    onClose();
  };
  return (
    <Dialog title={t("tips.title")} closeLabel={tc("errors.close")} onClose={close}>
      <ul className="m-0 flex list-none flex-col gap-4 p-0">
        {TIPS.map(({ icon: Icon, key }) => (
          <li key={key} className="flex gap-4">
            <span className="flex size-10 shrink-0 items-center justify-center rounded-full bg-bg-layer text-icon">
              <Icon size={20} strokeWidth={1.5} />
            </span>
            <div className="min-w-0">
              <div className="t-body-strong text-text-primary">{t(`tips.${key}.title`)}</div>
              <div className="t-caption text-text-secondary">{t(`tips.${key}.text`)}</div>
            </div>
          </li>
        ))}
      </ul>
      <Button variant="primary" onClick={close} className="self-start">
        {t("tips.ok")}
      </Button>
    </Dialog>
  );
}
