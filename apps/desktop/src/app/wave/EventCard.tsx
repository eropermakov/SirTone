import { CalendarHeart, MoreHorizontal } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { type EventAnswer, eventAnswer, useEventOffer } from "@/shared/ipc/events";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Menu } from "@/shared/ui/Menu";
import { notify } from "@/shared/ui/notify";

/**
 * The event card on «Моя волна» (T5.14, FR-EVT-004, `14` §11.3): «Скоро Хэллоуин» / «Сегодня …»,
 * «Включить» starts the event wave (theme and effect for the window), «Позже» asks again
 * tomorrow, «Ещё» → «Не в этом году» · «Никогда не предлагать». Rust shows ≤ 1 a day. When the
 * library has few direct matches the card says so honestly (AT-23).
 */
export function EventCard() {
  const { t } = useTranslation("shell");
  const { offer } = useEventOffer();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const id = offer.id;
  if (!id) return null;
  const name = t(`events.name.${id}`);
  const title =
    offer.daysToPeak > 0
      ? t("events.soon", { event: name })
      : offer.daysToPeak === 0
        ? t("events.today", { event: name })
        : t("events.now", { event: name });
  const answer = (a: EventAnswer) => {
    eventAnswer(id, a).catch(() => notify(t("events.failed")));
  };
  return (
    <div
      data-testid="event-card"
      className={cn(
        "relative flex max-w-xl flex-wrap items-center gap-3 rounded-(--radius-lg) border bg-bg-layer px-4 py-3 text-left",
        offer.strong ? "border-accent-text" : "border-border",
      )}
    >
      <CalendarHeart size={20} strokeWidth={1.5} className="shrink-0 text-accent-text" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="t-body-strong text-text-primary">{title}</span>
        <span className="t-caption text-text-secondary">
          {offer.atmospheric ? t("events.atmospheric") : t("events.offerText")}
        </span>
      </div>
      <Button variant="primary" onClick={() => answer("accept")}>
        {t("events.accept")}
      </Button>
      <Button variant="secondary" onClick={() => answer("later")}>
        {t("events.later")}
      </Button>
      <button
        type="button"
        aria-label={t("events.more")}
        onClick={(e) => setMenu({ x: e.clientX, y: e.clientY })}
        className="flex size-10 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
      >
        <MoreHorizontal size={20} strokeWidth={1.5} />
      </button>
      {menu && (
        <Menu
          at={menu}
          label={t("events.more")}
          onClose={() => setMenu(null)}
          items={[
            { id: "year", label: t("events.notThisYear"), onSelect: () => answer("year") },
            {
              id: "never",
              label: t("events.never"),
              danger: true,
              onSelect: () => answer("never"),
            },
          ]}
        />
      )}
    </div>
  );
}
