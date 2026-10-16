import { Check, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";
import { CollectionSummary } from "@/app/library/CollectionScreen";
import type { LibraryStatus } from "@/shared/ipc/library";
import { cn } from "@/shared/lib/utils";
import { ACCENT_SLUGS, type AccentSource, type ThemePref } from "@/shared/theme/theme";

type Props = {
  theme: ThemePref;
  accent: AccentSource;
  onTheme: (t: ThemePref) => void;
  onAccent: (a: AccentSource) => void;
  /** «Хранилище» (FR-DATA-004, FR-ONB-003): library folder and counters. */
  library?: LibraryStatus;
  onChooseLibrary?: () => void;
};

const THEMES: { id: ThemePref; key: string }[] = [
  { id: "system", key: "settings.themeSystem" },
  { id: "dark", key: "settings.themeDark" },
  { id: "light", key: "settings.themeLight" },
];

/** Settings → Внешний вид: theme + AccentPicker (DESIGN.md §9), applied instantly, no «Сохранить». */
export function SettingsScreen({
  theme,
  accent,
  onTheme,
  onAccent,
  library,
  onChooseLibrary,
}: Props) {
  const { t } = useTranslation("shell");
  const customHex = accent.kind === "custom" ? accent.hex : "#3b82f6";

  return (
    <section className="flex max-w-3xl flex-col gap-12">
      <h1 className="t-page-title text-primary-text">{t("settings.title")}</h1>

      <div className="flex flex-col gap-4">
        <h2 className="t-shelf-title text-primary-text">{t("settings.appearance")}</h2>

        <div className="flex flex-col gap-3 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
          <span className="t-card-title text-text-primary">{t("settings.theme")}</span>
          <fieldset
            className="m-0 border-0 p-0 flex flex-wrap gap-2"
            aria-label={t("settings.theme")}
          >
            {THEMES.map((it) => (
              <button
                key={it.id}
                type="button"
                aria-pressed={theme === it.id}
                onClick={() => onTheme(it.id)}
                className={cn(
                  "t-menu h-(--chip-height) rounded-full px-3",
                  theme === it.id
                    ? "bg-primary text-on-primary"
                    : "border border-border text-text-primary hover:bg-bg-hover",
                )}
              >
                {t(it.key)}
              </button>
            ))}
          </fieldset>
        </div>

        <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
          <span className="t-card-title text-text-primary">{t("settings.color")}</span>
          <fieldset
            className="m-0 border-0 p-0 flex flex-wrap gap-3"
            aria-label={t("settings.color")}
          >
            {ACCENT_SLUGS.map((slug) => {
              const selected = accent.kind === "preset" && accent.slug === slug;
              return (
                <button
                  key={slug}
                  type="button"
                  aria-pressed={selected}
                  aria-label={t(`accent.${slug}`)}
                  title={t(`accent.${slug}`)}
                  onClick={() => onAccent({ kind: "preset", slug })}
                  className={cn(
                    "relative flex size-8 items-center justify-center rounded-full",
                    selected && "ring-2 ring-text-primary ring-offset-[3px] ring-offset-bg-raised",
                  )}
                  style={
                    slug === "sirkov"
                      ? {
                          background: `linear-gradient(135deg, var(--accent-${slug}-primary) 50%, var(--accent-${slug}-fill) 50%)`,
                        }
                      : { background: `var(--accent-${slug}-fill)` }
                  }
                >
                  {selected && (
                    <Check
                      size={16}
                      strokeWidth={2}
                      style={{ color: `var(--accent-${slug}-on)` }}
                    />
                  )}
                </button>
              );
            })}
            <label
              title={t("settings.custom")}
              className={cn(
                "relative flex size-8 cursor-pointer items-center justify-center rounded-full border border-border text-icon",
                accent.kind === "custom" &&
                  "ring-2 ring-text-primary ring-offset-[3px] ring-offset-bg-raised",
              )}
              style={accent.kind === "custom" ? { background: customHex } : undefined}
            >
              <Plus size={16} strokeWidth={1.5} />
              <input
                type="color"
                aria-label={t("settings.custom")}
                value={customHex}
                onChange={(e) => onAccent({ kind: "custom", hex: e.target.value })}
                className="absolute inset-0 cursor-pointer opacity-0"
              />
            </label>
          </fieldset>

          <div className="mt-2 flex items-center gap-4 border-t border-separator pt-4">
            <span className="t-caption text-text-tertiary">{t("settings.preview")}</span>
            <span className="t-badge rounded-full bg-primary px-2 py-0.5 text-on-primary">
              {t("settings.badge")}
            </span>
            <button
              type="button"
              className="t-body-strong h-(--button-height) rounded-full bg-accent-fill px-5 text-accent-on"
            >
              {t("settings.previewButton")}
            </button>
            <div className="h-1 w-40 rounded-full bg-inactive">
              <div className="h-1 w-2/3 rounded-full bg-accent-text" />
            </div>
          </div>
        </div>
      </div>

      {library && (
        <div className="flex flex-col gap-2">
          <h2 className="t-shelf-title text-primary-text">{t("settingsStorage.title")}</h2>
          <div className="flex flex-col gap-3 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
            <span className="t-body-strong text-text-primary">{t("settingsStorage.folder")}</span>
            <span className="t-body break-all text-text-secondary">
              {library.root ?? t("settingsStorage.notChosen")}
            </span>
            {library.root && <CollectionSummary status={library} />}
            {!library.root && onChooseLibrary && (
              <button
                type="button"
                onClick={onChooseLibrary}
                className="t-body-strong w-fit text-accent-text"
              >
                {t("settingsStorage.change")}
              </button>
            )}
          </div>
        </div>
      )}

      <div className="flex flex-col gap-2">
        <h2 className="t-shelf-title text-primary-text">{t("settings.about")}</h2>
        <p className="t-body text-text-secondary">
          {t("settings.version", { version: __APP_VERSION__ })}
        </p>
      </div>
    </section>
  );
}
