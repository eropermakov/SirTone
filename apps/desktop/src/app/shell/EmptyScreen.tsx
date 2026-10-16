import { FolderPlus } from "lucide-react";
import { useTranslation } from "react-i18next";

/**
 * Empty state per DESIGN.md §9: primitives illustration + one line + one Primary button.
 * Import arrives in Stage 1, so the button is disabled with a «Скоро» badge (prototype 0.1).
 */
export function EmptyScreen({ screen }: { screen: string }) {
  const { t } = useTranslation("shell");
  return (
    <section className="flex h-full flex-col">
      <h1 className="t-page-title text-primary-text">{t(`empty.${screen}.title`)}</h1>
      <div className="flex flex-1 flex-col items-center justify-center gap-6 text-center">
        <div aria-hidden className="relative h-28 w-40">
          <div className="absolute top-2 left-4 size-20 rounded-full bg-artwork-placeholder" />
          <div className="absolute top-8 left-16 size-20 rounded-full border border-border bg-bg-raised" />
          <div className="absolute bottom-0 left-2 h-6 w-32 rounded-full bg-primary" />
        </div>
        <p className="t-body max-w-md text-text-secondary">{t(`empty.${screen}.text`)}</p>
        <div className="flex items-center gap-3">
          <button
            type="button"
            disabled
            className="t-body-strong flex h-(--button-height) items-center gap-2 rounded-full bg-accent-fill px-5 text-accent-on disabled:opacity-40"
          >
            <FolderPlus size={20} strokeWidth={1.5} />
            {t("empty.action")}
          </button>
          <span className="t-badge rounded-full border border-inactive px-2 py-0.5 text-text-secondary">
            {t("empty.soon")}
          </span>
        </div>
      </div>
    </section>
  );
}
