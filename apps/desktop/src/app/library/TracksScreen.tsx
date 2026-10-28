import { ArrowDown, ArrowUp, ArrowUpDown, Play } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { allTracks, type LibraryStatus, playTracks, type TrackItem } from "@/shared/ipc/library";
import { Button } from "@/shared/ui/Button";
import { Menu } from "@/shared/ui/Menu";
import { SkeletonRows } from "@/shared/ui/Skeleton";
import { CollectionSummary } from "./CollectionScreen";
import { loadSort, next, saveSort, sortTracks, TRACK_SORTS } from "./sort";
import { type GoTo, TrackList } from "./TrackList";

/**
 * «Все треки» (D-127; `01b` F-34: Apple Music «Песни», Windows Media Player «вся музыка»): every
 * playable track in one TrackList with «Слушать» and the sort chip; double click plays the
 * sorted list from that row.
 */
export function TracksScreen({
  status,
  version,
  playingId,
  onGo,
}: {
  status: LibraryStatus;
  version: number;
  playingId: number | null;
  onGo: GoTo;
}) {
  const { t } = useTranslation("shell");
  const [tracks, setTracks] = useState<TrackItem[] | null>(null);
  const [sort, setSort] = useState(() => loadSort("tracks", TRACK_SORTS, "title"));
  const [sortMenu, setSortMenu] = useState<{ x: number; y: number } | null>(null);

  // All pages (keyset, 500 per call); the first page shows at once. Refetch when the library changes.
  useEffect(() => {
    void version;
    let alive = true;
    allTracks((rows) => alive && setTracks(rows))
      .then((rows) => alive && setTracks(rows))
      .catch(() => alive && setTracks([]));
    return () => {
      alive = false;
    };
  }, [version]);

  // Sorting 100 000 rows on every render (playingId changes each track) would cost the budget.
  const sorted = useMemo(() => (tracks ? sortTracks(tracks, sort) : null), [tracks, sort]);
  const ids = useMemo(() => (sorted ?? []).map((x) => x.id), [sorted]);

  return (
    <section className="flex h-full min-h-0 flex-col gap-2">
      <div className="flex items-center justify-between gap-4">
        <h1 className="t-page-title text-primary-text">{t("empty.tracks.title")}</h1>
        <Button
          variant="ghost"
          onClick={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            setSortMenu({ x: r.right - 240, y: r.bottom + 4 });
          }}
          className="gap-2"
        >
          <ArrowUpDown size={16} strokeWidth={1.5} />
          {t(`sort.${sort.by}`)}
        </Button>
      </div>
      <CollectionSummary status={status} />
      {sortMenu && (
        <Menu
          at={sortMenu}
          label={t("sort.label")}
          onClose={() => setSortMenu(null)}
          items={TRACK_SORTS.map((by) => ({
            id: by,
            label: t(`sort.${by}`),
            icon: by === sort.by ? (sort.reverse ? ArrowUp : ArrowDown) : undefined,
            onSelect: () => {
              const s = next(sort, by);
              setSort(s);
              saveSort("tracks", s);
            },
          }))}
        />
      )}
      <div className="mt-4">
        <button
          type="button"
          disabled={ids.length === 0}
          onClick={() => playTracks(ids, 0, "collection")}
          className="t-body-strong flex h-(--button-height) items-center gap-2 rounded-full bg-accent-fill px-5 text-accent-on disabled:opacity-40"
        >
          <Play size={20} fill="currentColor" strokeWidth={1.5} />
          {t("collection.play")}
        </button>
      </div>
      {sorted === null ? (
        <div className="mt-2">
          <SkeletonRows />
        </div>
      ) : (
        <div className="mt-2 flex min-h-0 flex-1 flex-col">
          <TrackList
            tracks={sorted}
            playingId={playingId}
            label={t("empty.tracks.title")}
            onPlay={(i) => playTracks(ids, i, "collection")}
            context="collection"
            onGo={onGo}
          />
        </div>
      )}
    </section>
  );
}
