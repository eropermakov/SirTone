import { X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  EMPTY_QUEUE,
  getQueue,
  onQueueChanged,
  type QueueView,
  queueJump,
  queueRemove,
} from "@/shared/ipc/playback";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Cover } from "@/shared/ui/Cover";

/**
 * Queue panel (FR-PLAY-006; Spotify/Apple Music «Up Next», `01b` F-21). DESIGN.md has no queue
 * panel: neutral Panel styling + TrackRow-like rows with a 40 px cover (temporary, `06` §12).
 * Click plays from that row, the cross removes it; the playing row cannot be removed.
 */
export function QueuePanel({ onClose, trackKey }: { onClose: () => void; trackKey: unknown }) {
  const { t } = useTranslation("shell");
  const [q, setQ] = useState<QueueView>(EMPTY_QUEUE);
  const load = useCallback(() => {
    getQueue().then(setQ);
  }, []);

  // Refetch on queue edits and whenever the playing track changes.
  // biome-ignore lint/correctness/useExhaustiveDependencies: trackKey is the refetch trigger
  useEffect(load, [load, trackKey]);
  useEffect(() => {
    let off: (() => void) | undefined;
    let alive = true;
    onQueueChanged(load).then((f) => {
      if (alive) off = f;
      else f();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, [load]);

  return (
    <aside
      aria-label={t("queue.title")}
      className="flex w-90 shrink-0 flex-col gap-2 rounded-(--radius-xl) border border-border bg-bg-raised p-4"
    >
      <div className="flex items-center justify-between">
        <h2 className="t-card-title text-text-primary">{t("queue.title")}</h2>
        <Button variant="icon" label={t("queue.close")} onClick={onClose}>
          <X size={20} strokeWidth={1.5} />
        </Button>
      </div>
      {q.items.length === 0 ? (
        <p className="t-body py-6 text-center text-text-secondary">{t("queue.empty")}</p>
      ) : (
        <ol className="m-0 flex min-h-0 flex-1 list-none flex-col overflow-auto p-0">
          {q.items.map((it, i) => {
            const current = i === q.position;
            return (
              // biome-ignore lint/suspicious/noArrayIndexKey: the same track may be queued twice
              <li key={i} className="group">
                {(current || i === q.position + 1) && (
                  <div className="t-group-label px-3 pt-2 pb-1 text-text-secondary">
                    {current ? t("queue.now") : t("queue.next")}
                  </div>
                )}
                <div className="flex h-(--row-height) items-center gap-3 rounded-(--radius-md) px-2 hover:bg-bg-hover">
                  <button
                    type="button"
                    aria-current={current ? "true" : undefined}
                    onClick={() => queueJump(i)}
                    className="flex min-w-0 flex-1 items-center gap-3 text-left"
                  >
                    <Cover hash={it.cover} px={40} />
                    <div className="min-w-0">
                      <div
                        className={cn(
                          "t-body truncate",
                          current
                            ? "text-accent-text"
                            : i < q.position
                              ? "text-text-tertiary"
                              : "text-text-primary",
                        )}
                      >
                        {it.title}
                      </div>
                      <div className="t-caption truncate text-text-secondary">{it.artists}</div>
                    </div>
                  </button>
                  {!current && (
                    <Button
                      variant="icon"
                      label={t("queue.remove")}
                      onClick={() => queueRemove(i)}
                      className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                    >
                      <X size={16} strokeWidth={1.5} />
                    </Button>
                  )}
                </div>
              </li>
            );
          })}
        </ol>
      )}
    </aside>
  );
}
