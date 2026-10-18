import { ArrowLeft, Play } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  collectionGenres,
  type GenreItem,
  genreTracks,
  playTracks,
  type TrackItem,
} from "@/shared/ipc/library";
import { Skeleton } from "@/shared/ui/Skeleton";
import { type GoTo, TrackList } from "./TrackList";

/**
 * «Жанры» (FR-LIB-002; `01b` F-34): genres from tags, spellings merged; a tile per genre with
 * its track count and Play, the genre page lists its tracks. Genres by model / online and
 * «Настроения» come with analysis (Stage 3, FR-LIB-009).
 */
export function GenresScreen({
  version,
  playingId,
  open,
  onOpen,
  onBack,
  onGo,
}: {
  version: number;
  playingId: number | null;
  /** The opened genre (navigation stack). */
  open: GenreItem | null;
  onOpen: (g: GenreItem) => void;
  onBack: () => void;
  onGo: GoTo;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [genres, setGenres] = useState<GenreItem[] | null>(null);
  const [tracks, setTracks] = useState<TrackItem[]>([]);

  useEffect(() => {
    void version;
    collectionGenres()
      .then(setGenres)
      .catch(() => setGenres([]));
  }, [version]);

  useEffect(() => {
    void version;
    if (!open) return;
    genreTracks(open.key)
      .then(setTracks)
      .catch(() => setTracks([]));
  }, [open, version]);

  const play = (key: string) =>
    genreTracks(key).then((tr) => {
      if (tr.length > 0)
        playTracks(
          tr.map((x) => x.id),
          0,
          "collection",
        );
    });

  if (open) {
    const ids = tracks.map((x) => x.id);
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
          <div className="min-w-0 flex-1">
            <span className="t-caption text-text-secondary">{t("genres.kind")}</span>
            <h1 className="t-page-title truncate text-primary-text">{open.name}</h1>
            <p className="t-caption text-text-secondary">
              {tc("count.tracks", { count: tracks.length })}
            </p>
          </div>
          <button
            type="button"
            disabled={ids.length === 0}
            onClick={() => playTracks(ids, 0, "collection")}
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
          onPlay={(i) => playTracks(ids, i, "collection")}
          context="collection"
          onGo={onGo}
        />
      </section>
    );
  }

  return (
    <section className="flex h-full min-h-0 flex-col gap-4">
      <h1 className="t-page-title text-primary-text">{t("genres.title")}</h1>
      {genres === null ? (
        <div className="flex flex-wrap gap-3">
          {Array.from({ length: 8 }, (_, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: placeholders
            <Skeleton key={i} className="h-20 w-56 rounded-(--radius-lg)" />
          ))}
        </div>
      ) : genres.length === 0 ? (
        <p className="t-body max-w-md text-text-secondary">{t("genres.empty")}</p>
      ) : (
        <ul
          aria-label={t("genres.title")}
          className="m-0 flex list-none flex-wrap gap-3 overflow-auto p-0"
        >
          {genres.map((g) => (
            <li key={g.key} className="group relative">
              <button
                type="button"
                onClick={() => onOpen(g)}
                className="flex h-20 w-56 flex-col justify-center rounded-(--radius-lg) border border-border bg-bg-raised px-4 text-left hover:bg-bg-hover"
              >
                <span className="t-body-strong truncate text-text-primary">{g.name}</span>
                <span className="t-caption text-text-secondary">
                  {tc("count.tracks", { count: g.tracks })}
                </span>
              </button>
              <button
                type="button"
                aria-label={t("genres.play", { name: g.name })}
                onClick={() => void play(g.key)}
                className="absolute top-5 right-3 flex size-10 items-center justify-center rounded-full bg-accent-fill text-accent-on opacity-0 transition-opacity duration-(--dur-fast) group-hover:opacity-100 focus-visible:opacity-100"
              >
                <Play size={18} fill="currentColor" strokeWidth={1.5} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
