import { Infinity as InfinityIcon, Minus, Plus, X } from "lucide-react";
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
import { addToPlaylist, continuationExclude, continuationSet } from "@/shared/ipc/playlists";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Cover } from "@/shared/ui/Cover";
import { notify } from "@/shared/ui/notify";

/**
 * Queue panel (FR-PLAY-006; Spotify/Apple Music «Up Next», `01b` F-21). DESIGN.md has no queue
 * panel: neutral Panel styling + TrackRow-like rows with a 40 px cover (temporary, `06` §12).
 * Click plays from that row, the cross removes it; the playing row cannot be removed. ∞ (T3.14,
 * FR-PLS-005; Apple Music «∞» next to «Playing Next», `01b` F-14) continues the playlist or list
 * with similar tracks from the library, shown under a divider with «+» (into the playlist) and
 * «−» (never offer again).
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
        <div className="flex items-center gap-1">
          {q.autoplay !== null && (
            <Button
              variant="icon"
              label={t("continuation.toggle")}
              aria-pressed={q.autoplay}
              title={t(q.autoplay ? "continuation.on" : "continuation.off")}
              onClick={() => {
                const next = !q.autoplay;
                setQ({ ...q, autoplay: next }); // optimistic (DESIGN.md §1)
                continuationSet(q.playlist, next).catch(() => {
                  load();
                  notify(t("continuation.failed"));
                });
              }}
              className={q.autoplay ? "text-accent-text" : "text-text-tertiary"}
            >
              <InfinityIcon size={20} strokeWidth={1.5} />
            </Button>
          )}
          <Button variant="icon" label={t("queue.close")} onClick={onClose}>
            <X size={20} strokeWidth={1.5} />
          </Button>
        </div>
      </div>
      {q.items.length === 0 ? (
        <p className="t-body py-6 text-center text-text-secondary">{t("queue.empty")}</p>
      ) : (
        <ol className="m-0 flex min-h-0 flex-1 list-none flex-col overflow-auto p-0">
          {q.items.map((it, i) => {
            const current = i === q.position;
            const firstRecommended = it.recommended && !q.items[i - 1]?.recommended;
            return (
              // biome-ignore lint/suspicious/noArrayIndexKey: the same track may be queued twice
              <li key={i} className="group">
                {firstRecommended && (
                  <div className="t-group-label px-3 pt-3 pb-1 text-text-secondary">
                    {t("continuation.title")}
                  </div>
                )}
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
                    <Cover hash={it.cover} px={40} art={it.art} />
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
                  {it.recommended && it.trackId !== null && (
                    <>
                      {q.playlist !== null && (
                        <Button
                          variant="icon"
                          label={t("continuation.add")}
                          onClick={() => {
                            const pl = q.playlist;
                            const tr = it.trackId;
                            if (pl === null || tr === null) return;
                            addToPlaylist(pl, [tr])
                              .then(() => notify(t("continuation.added", { title: it.title })))
                              .catch(() => notify(t("continuation.failed")));
                          }}
                          className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                        >
                          <Plus size={16} strokeWidth={1.5} />
                        </Button>
                      )}
                      {!current && (
                        <Button
                          variant="icon"
                          label={t("continuation.exclude")}
                          onClick={() => {
                            const tr = it.trackId;
                            if (tr === null) return;
                            continuationExclude(q.playlist, tr)
                              .then(() => notify(t("continuation.excluded", { title: it.title })))
                              .catch(() => notify(t("continuation.failed")));
                          }}
                          className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
                        >
                          <Minus size={16} strokeWidth={1.5} />
                        </Button>
                      )}
                    </>
                  )}
                  {!current && !it.recommended && (
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
