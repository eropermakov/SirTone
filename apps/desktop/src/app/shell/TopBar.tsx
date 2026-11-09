import { ChevronLeft, ChevronRight } from "lucide-react";
import { useTranslation } from "react-i18next";

/**
 * The bar above the content (DESIGN.md §13 «Каркас», `frame.header`): the «назад, вперёд» capsule
 * and the name of the subsection, a 1 px line below. Glass over the visualiser (FR-LOOK-004/007).
 */
export function TopBar({
  title,
  canBack,
  canForward,
  onBack,
  onForward,
}: {
  title: string;
  canBack: boolean;
  canForward: boolean;
  onBack: () => void;
  onForward: () => void;
}) {
  const { t } = useTranslation("nav");
  const btn =
    "flex h-(--button-height) w-10 items-center justify-center text-icon transition-colors duration-(--dur-fast) enabled:hover:text-text-primary disabled:opacity-40";
  return (
    <header className="glass relative z-10 flex h-(--titlebar-height) shrink-0 items-center gap-3 border-b border-separator px-4">
      <div className="flex overflow-hidden rounded-full bg-bg-hover">
        <button
          type="button"
          onClick={onBack}
          disabled={!canBack}
          aria-label={t("back")}
          className={btn}
        >
          <ChevronLeft size={20} strokeWidth={1.5} />
        </button>
        <button
          type="button"
          onClick={onForward}
          disabled={!canForward}
          aria-label={t("forward")}
          className={btn}
        >
          <ChevronRight size={20} strokeWidth={1.5} />
        </button>
      </div>
      <span className="t-card-title min-w-0 flex-1 truncate text-text-primary">{title}</span>
    </header>
  );
}
