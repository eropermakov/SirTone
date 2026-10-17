import { ArrowLeft, Play } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  type AlbumItem,
  type ArtistItem,
  albumsPage,
  albumTracks,
  artistsPage,
  artistTracks,
  type LibraryStatus,
  playTracks,
  type TrackItem,
} from "@/shared/ipc/library";
import { queueAppend, queuePlayNext } from "@/shared/ipc/playback";
import { Cover } from "@/shared/ui/Cover";
import { notify } from "@/shared/ui/notify";
import { ArtistRemove } from "./ArtistRemove";
import { type Card, CardGrid } from "./CardGrid";
import { type GoTo, TrackList } from "./TrackList";

type Kind = "albums" | "artists";
/** An opened album or artist: part of the navigation stack (T2.1). */
export type Open = {
  kind: Kind;
  id: number;
  title: string;
  subtitle: string;
  cover: string | null;
};

/** «Вся коллекция: N треков · Ч ч М мин · X ГБ» (docs/06 §4, FR-LIB-001). */
export function CollectionSummary({ status }: { status: LibraryStatus }) {
  const { t } = useTranslation("shell");
  const bytes = status.bytes ?? 0;
  const gb = bytes / 1024 ** 3;
  const size =
    gb >= 1
      ? t("collection.sizeGb", { value: gb.toFixed(1) })
      : t("collection.sizeMb", { value: Math.round(bytes / 1024 ** 2) });
  return (
    <p className="t-caption text-text-secondary">
      {t("collection.summary", {
        tracks: status.tracks,
        hours: Math.floor(status.durationSec / 3600),
        minutes: Math.floor((status.durationSec % 3600) / 60),
        size,
      })}
    </p>
  );
}

/**
 * Stage 1 collection lists (T1.27 debug UI in DESIGN.md style): albums or artists → their tracks.
 * Covers, grids and shelves come with Stage 2.
 */
export function CollectionScreen({
  kind,
  status,
  version,
  playingId,
  open,
  onOpen,
  onBack,
}: {
  kind: Kind;
  status: LibraryStatus;
  version: number;
  playingId: number | null;
  /** The album/artist shown instead of the list (from the navigation stack). */
  open: Open | null;
  onOpen: (o: Open) => void;
  onBack: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [albums, setAlbums] = useState<AlbumItem[]>([]);
  const [artists, setArtists] = useState<ArtistItem[]>([]);
  // Skeletons until the first page arrives (DESIGN.md §9: never a spinner).
  const [loaded, setLoaded] = useState(false);
  const [tracks, setTracks] = useState<TrackItem[]>([]);
  const [removing, setRemoving] = useState<Card | null>(null);
  const closeRemove = useCallback(() => setRemoving(null), []);
  const go: GoTo = (k, id, title) => onOpen({ kind: k, id, title, subtitle: "", cover: null });

  // Lists: all pages (keyset, 200 per call); refetch when the library changes.
  useEffect(() => {
    let alive = true;
    const load = async () => {
      if (kind === "albums") {
        const all: AlbumItem[] = [];
        let page = await albumsPage(null);
        all.push(...page.rows);
        while (page.next && alive) {
          page = await albumsPage(page.next);
          all.push(...page.rows);
        }
        if (alive) {
          setAlbums(all);
          setLoaded(true);
        }
      } else {
        const all: ArtistItem[] = [];
        let page = await artistsPage(null);
        all.push(...page.rows);
        while (page.next && alive) {
          page = await artistsPage(page.next);
          all.push(...page.rows);
        }
        if (alive) {
          setArtists(all);
          setLoaded(true);
        }
      }
    };
    void version;
    load().catch(() => {});
    return () => {
      alive = false;
    };
  }, [kind, version]);

  useEffect(() => {
    void version;
    if (!open) return;
    const fetcher = open.kind === "albums" ? albumTracks : artistTracks;
    fetcher(open.id)
      .then(setTracks)
      .catch(() => setTracks([]));
  }, [open, version]);

  if (open) {
    const ids = tracks.map((x) => x.id);
    const context = open.kind === "albums" ? "album" : "artist";
    return (
      <section className="flex h-full min-h-0 flex-col gap-4">
        <button
          type="button"
          onClick={onBack}
          className="t-body flex w-fit items-center gap-2 text-text-secondary hover:text-text-primary"
        >
          <ArrowLeft size={20} strokeWidth={1.5} />
          {t("collection.back")}
        </button>
        <div className="flex items-end gap-6">
          {open.kind === "albums" && (
            <Cover hash={open.cover} px={184} rounded="md" className="shadow-(--shadow-artwork)" />
          )}
          <div className="min-w-0 flex-1">
            <h1 className="t-page-title truncate text-primary-text">{open.title}</h1>
            <p className="t-caption text-text-secondary">{open.subtitle}</p>
          </div>
          <button
            type="button"
            disabled={ids.length === 0}
            onClick={() => playTracks(ids, 0, context)}
            className="t-body-strong flex h-(--button-height) shrink-0 items-center gap-2 rounded-full bg-accent-fill px-5 text-accent-on disabled:opacity-40"
          >
            <Play size={20} fill="currentColor" strokeWidth={1.5} />
            {t("collection.play")}
          </button>
        </div>
        <TrackList
          tracks={tracks}
          playingId={playingId}
          label={t("collection.trackList")}
          onPlay={(i) => playTracks(ids, i, context)}
          context={context}
          onGo={go}
        />
      </section>
    );
  }

  const cards: Card[] | null = !loaded
    ? null
    : kind === "albums"
      ? albums.map((a) => ({
          id: a.id,
          title: a.title,
          subtitle: [a.artist, a.year].filter(Boolean).join(" · "),
          cover: a.cover,
          count: a.tracks,
        }))
      : artists.map((a) => ({
          id: a.id,
          title: a.name,
          subtitle: tc("count.tracks", { count: a.tracks }),
          cover: null,
          count: a.tracks,
        }));
  const tracksOf = (c: Card) => (kind === "albums" ? albumTracks(c.id) : artistTracks(c.id));
  const ctx = kind === "albums" ? "album" : "artist";

  return (
    <section className="flex h-full min-h-0 flex-col gap-2">
      <h1 className="t-page-title text-primary-text">{t(`empty.${kind}.title`)}</h1>
      <CollectionSummary status={status} />
      <div className="mt-4 flex min-h-0 flex-1 flex-col">
        <CardGrid
          kind={kind}
          cards={cards}
          label={t(`empty.${kind}.title`)}
          onOpen={(c) =>
            onOpen({
              kind,
              id: c.id,
              title: c.title,
              subtitle: kind === "albums" ? c.subtitle : "",
              cover: c.cover,
            })
          }
          onPlay={(c) =>
            tracksOf(c).then((tr) => {
              if (tr.length > 0)
                playTracks(
                  tr.map((x) => x.id),
                  0,
                  ctx,
                );
            })
          }
          onPlayNext={(c) =>
            tracksOf(c).then((tr) => {
              queuePlayNext(
                tr.map((x) => x.id),
                ctx,
              );
              notify(t("trackMenu.queuedNext", { title: c.title }));
            })
          }
          onAppend={(c) =>
            tracksOf(c).then((tr) => {
              queueAppend(
                tr.map((x) => x.id),
                ctx,
              );
              notify(t("trackMenu.queued", { title: c.title }));
            })
          }
          onRemove={kind === "artists" ? setRemoving : undefined}
        />
      </div>
      {removing && <ArtistRemove artist={removing} onClose={closeRemove} />}
    </section>
  );
}
