import { Infinity as InfinityIcon, Minus, Plus } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TrackItem } from "@/shared/ipc/library";
import {
  addToPlaylist,
  continuationExclude,
  continuationPreview,
  continuationSet,
  onPlaylistsChanged,
} from "@/shared/ipc/playlists";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { notify } from "@/shared/ui/notify";

/**
 * «Мои треки → разделитель → рекомендованные» under a playlist (T3.14, FR-PLS-003; `06`
 * «Страница плейлиста (гибридный)»; `01b` F-14 — Roon, Apple Music ∞, Spotify «+»/«−»). The
 * same sequence ∞ plays after the playlist, closest first. DESIGN.md has no divider of this
 * kind: TrackRow-like rows and a caption divider (temporary, `06` §12).
 */
export function Recommended({
  playlist,
  on,
  version,
}: {
  playlist: number;
  /** ∞ of the playlist. */
  on: boolean;
  /** LibraryChanged counter. */
  version: number;
}) {
  const { t } = useTranslation("shell");
  const [enabled, setEnabled] = useState(on);
  const [items, setItems] = useState<TrackItem[] | null>(null);

  useEffect(() => setEnabled(on), [on]);

  const load = useCallback(() => {
    continuationPreview(playlist)
      .then(setItems)
      .catch(() => setItems([]));
  }, [playlist]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: version and the switch refetch
  useEffect(load, [load, version, enabled]);
  useEffect(() => {
    let off: (() => void) | undefined;
    let alive = true;
    void onPlaylistsChanged((id) => {
      if (id === playlist) load();
    }).then((f) => {
      if (alive) off = f;
      else f();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, [playlist, load]);

  const toggle = () => {
    const next = !enabled;
    setEnabled(next); // optimistic (DESIGN.md §1)
    continuationSet(playlist, next).catch(() => {
      setEnabled(!next);
      notify(t("continuation.failed"));
    });
  };

  const drop = (tr: TrackItem) => setItems((list) => (list ?? []).filter((x) => x.id !== tr.id));

  const plus = (tr: TrackItem) => {
    drop(tr);
    addToPlaylist(playlist, [tr.id])
      .then(() => notify(t("continuation.added", { title: tr.title })))
      .catch(() => {
        load();
        notify(t("continuation.failed"));
      });
  };

  const minus = (tr: TrackItem) => {
    drop(tr);
    continuationExclude(playlist, tr.id)
      .then(() => notify(t("continuation.excluded", { title: tr.title })))
      .catch(() => {
        load();
        notify(t("continuation.failed"));
      });
  };

  return (
    <section aria-label={t("continuation.title")} className="flex flex-col gap-1 pt-4 pb-6">
      <div className="flex items-center gap-3 px-3">
        <span aria-hidden className="h-px flex-1 bg-border" />
        <span className="t-caption text-text-secondary">{t("continuation.title")}</span>
        <span aria-hidden className="h-px flex-1 bg-border" />
        <Button
          variant="icon"
          label={t("continuation.toggle")}
          aria-pressed={enabled}
          title={t(enabled ? "continuation.on" : "continuation.off")}
          onClick={toggle}
          className={cn(enabled ? "text-accent-text" : "text-text-tertiary")}
        >
          <InfinityIcon size={20} strokeWidth={1.5} />
        </Button>
      </div>
      {!enabled ? (
        <p className="t-caption px-3 py-2 text-text-secondary">{t("continuation.offHint")}</p>
      ) : items === null ? (
        <div aria-hidden className="mx-3 h-(--row-height) rounded-(--radius-md) bg-bg-hover" />
      ) : items.length === 0 ? (
        <p className="t-caption px-3 py-2 text-text-secondary">{t("continuation.none")}</p>
      ) : (
        <ul aria-label={t("continuation.title")} className="m-0 list-none p-0">
          {items.map((tr) => (
            <li
              key={tr.id}
              data-recommended={tr.id}
              className="group flex h-(--row-height) items-center gap-3 rounded-(--radius-md) pr-2 pl-14 hover:bg-bg-hover"
            >
              <div className="min-w-0 flex-1">
                <div className="t-body truncate text-text-primary">{tr.title}</div>
                <div className="t-caption truncate text-text-secondary">{tr.artists}</div>
              </div>
              <Button variant="icon" label={t("continuation.add")} onClick={() => plus(tr)}>
                <Plus size={16} strokeWidth={1.5} />
              </Button>
              <Button variant="icon" label={t("continuation.exclude")} onClick={() => minus(tr)}>
                <Minus size={16} strokeWidth={1.5} />
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
