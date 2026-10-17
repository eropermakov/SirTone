import { ListMusic, Play, Search, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type Card, CardGrid } from "@/app/library/CardGrid";
import { type GoTo, TrackList } from "@/app/library/TrackList";
import {
  type AlbumItem,
  type ArtistItem,
  albumTracks,
  artistTracks,
  playTracks,
  type TrackItem,
} from "@/shared/ipc/library";
import { queueAppend, queuePlayNext } from "@/shared/ipc/playback";
import { type SearchResults, search } from "@/shared/ipc/search";
import { Chip } from "@/shared/ui/Chip";
import { Cover } from "@/shared/ui/Cover";
import { notify } from "@/shared/ui/notify";

const GROUPS = ["all", "tracks", "artists", "albums", "playlists"] as const;
type Group = (typeof GROUPS)[number];
/** Track rows under «Все» next to the best match (Spotify / Apple Music). */
const TOP_TRACKS = 5;
const ROW = 52;
/** Typing pause before a query (results ≤ 400 ms after the last key, DESIGN.md §1). */
const DEBOUNCE_MS = 80;

/** The last query survives «назад» to the search screen. */
let lastQuery = "";

const albumCard = (a: AlbumItem): Card => ({
  id: a.id,
  title: a.title,
  subtitle: [a.artist, a.year].filter(Boolean).join(" · "),
  cover: a.cover,
});

/**
 * Search (T2.13, FR-SRCH-001/002; `01b` F-17): results as you type, «лучшее совпадение» and
 * groups — tracks, artists, albums, playlists — with filter chips (DESIGN.md §9 Chip).
 */
export function SearchScreen({
  playingId,
  version,
  onGo,
  onOpenPlaylist,
}: {
  playingId: number | null;
  version: number;
  onGo: GoTo;
  onOpenPlaylist: (id: number) => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [query, setQuery] = useState(lastQuery);
  const [group, setGroup] = useState<Group>("all");
  const [res, setRes] = useState<SearchResults | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const seq = useRef(0);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  useEffect(() => {
    void version;
    lastQuery = query;
    const q = query.trim();
    if (!q) {
      setRes(null);
      return;
    }
    const n = ++seq.current;
    const timer = setTimeout(() => {
      search(q)
        .then((r) => {
          if (n === seq.current) setRes(r);
        })
        .catch(() => {});
    }, DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query, version]);

  const kindOf = (k: "albums" | "artists") => (k === "albums" ? "album" : "artist");
  const tracksOf = (k: "albums" | "artists", id: number) =>
    k === "albums" ? albumTracks(id) : artistTracks(id);
  const grid = (k: "albums" | "artists", cards: Card[], label: string) => (
    <CardGrid
      kind={k}
      cards={cards}
      label={label}
      onOpen={(c) => onGo(k, c.id, c.title)}
      onPlay={(c) =>
        tracksOf(k, c.id).then((tr) => {
          if (tr.length > 0)
            playTracks(
              tr.map((x) => x.id),
              0,
              kindOf(k),
            );
        })
      }
      onPlayNext={(c) =>
        tracksOf(k, c.id).then((tr) => {
          queuePlayNext(
            tr.map((x) => x.id),
            kindOf(k),
          );
          notify(t("trackMenu.queuedNext", { title: c.title }));
        })
      }
      onAppend={(c) =>
        tracksOf(k, c.id).then((tr) => {
          queueAppend(
            tr.map((x) => x.id),
            kindOf(k),
          );
          notify(t("trackMenu.queued", { title: c.title }));
        })
      }
    />
  );

  const best = res?.best ?? null;
  // The best track leads the track list when it is played from there.
  const allTracks: TrackItem[] = res
    ? best?.kind === "track"
      ? [best.item, ...res.tracks]
      : res.tracks
    : [];
  const artistCards = (res?.artists ?? []).map(
    (a: ArtistItem): Card => ({
      id: a.id,
      title: a.name,
      subtitle: tc("count.tracks", { count: a.tracks }),
      cover: null,
    }),
  );
  const albumCards = (res?.albums ?? []).map(albumCard);
  const playlists = res?.playlists ?? [];
  const empty =
    res !== null &&
    !best &&
    allTracks.length === 0 &&
    artistCards.length === 0 &&
    albumCards.length === 0 &&
    playlists.length === 0;
  const artistGrid = grid("artists", artistCards, t("search.artists"));
  const albumGrid = grid("albums", albumCards, t("search.albums"));
  const playAll = (i: number) =>
    playTracks(
      allTracks.map((x) => x.id),
      i,
      "search",
    );
  const tracksBox = (list: TrackItem[], fixed: boolean) => (
    <div
      className={fixed ? "flex" : "flex min-h-0 flex-1"}
      style={fixed ? { height: list.length * ROW } : undefined}
    >
      <TrackList
        tracks={list}
        playingId={playingId}
        label={t("search.tracks")}
        onPlay={(i) => playAll(i)}
        context="search"
        onGo={onGo}
      />
    </div>
  );

  return (
    <section className="flex h-full min-h-0 flex-col gap-4">
      <div className="flex h-12 shrink-0 items-center gap-3 rounded-(--radius-lg) border border-border bg-bg-raised px-4 focus-within:border-text-secondary">
        <Search size={20} strokeWidth={1.5} className="text-icon" />
        <input
          ref={input}
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && query) {
              e.stopPropagation();
              setQuery("");
            }
          }}
          placeholder={t("search.placeholder")}
          aria-label={t("search.label")}
          className="t-body min-w-0 flex-1 bg-transparent text-text-primary outline-none placeholder:text-text-tertiary [&::-webkit-search-cancel-button]:hidden"
        />
        {query && (
          <button
            type="button"
            onClick={() => {
              setQuery("");
              input.current?.focus();
            }}
            aria-label={t("search.clear")}
            className="flex size-8 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
          >
            <X size={16} strokeWidth={1.5} />
          </button>
        )}
      </div>
      {res && !empty && (
        <div className="flex shrink-0 flex-wrap gap-2">
          {GROUPS.map((g) => (
            <Chip key={g} active={group === g} onClick={() => setGroup(g)}>
              {t(`search.group.${g}`)}
            </Chip>
          ))}
        </div>
      )}
      {!res ? (
        <p className="t-body text-text-secondary">{t("search.hint")}</p>
      ) : empty ? (
        <p role="status" className="t-body text-text-secondary">
          {t("search.nothing", { query: query.trim() })}
        </p>
      ) : group === "tracks" ? (
        tracksBox(allTracks, false)
      ) : group === "artists" ? (
        artistGrid
      ) : group === "albums" ? (
        albumGrid
      ) : group === "playlists" ? (
        <PlaylistHits items={playlists} onOpen={onOpenPlaylist} />
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-6 overflow-auto">
          {(best || res.tracks.length > 0) && (
            <div className="flex shrink-0 gap-6">
              {best && (
                <div className="flex w-80 shrink-0 flex-col gap-2">
                  <h2 className="t-card-title text-text-primary">{t("search.best")}</h2>
                  <BestCard
                    best={best}
                    onOpen={() => {
                      if (best.kind === "artist") onGo("artists", best.item.id, best.item.name);
                      else if (best.kind === "album") onGo("albums", best.item.id, best.item.title);
                      else playAll(0);
                    }}
                  />
                </div>
              )}
              {allTracks.length > (best?.kind === "track" ? 1 : 0) && (
                <div className="flex min-w-0 flex-1 flex-col gap-2">
                  <h2 className="t-card-title text-text-primary">{t("search.tracks")}</h2>
                  {tracksBox(allTracks.slice(0, TOP_TRACKS), true)}
                </div>
              )}
            </div>
          )}
          {artistCards.length > 0 && <Row title={t("search.artists")}>{artistGrid}</Row>}
          {albumCards.length > 0 && <Row title={t("search.albums")}>{albumGrid}</Row>}
          {playlists.length > 0 && (
            <div className="flex shrink-0 flex-col gap-2">
              <h2 className="t-card-title text-text-primary">{t("search.playlists")}</h2>
              <PlaylistHits items={playlists} onOpen={onOpenPlaylist} />
            </div>
          )}
        </div>
      )}
    </section>
  );
}

/** One row of cards (CardGrid keeps 184 px cards; the rest of the group is a chip away). */
function Row({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="flex shrink-0 flex-col gap-2">
      <h2 className="t-card-title text-text-primary">{title}</h2>
      <div className="flex h-[264px] overflow-hidden">{children}</div>
    </div>
  );
}

function BestCard({
  best,
  onOpen,
}: {
  best: NonNullable<SearchResults["best"]>;
  onOpen: () => void;
}) {
  const { t } = useTranslation("shell");
  const title = best.kind === "artist" ? best.item.name : best.item.title;
  const sub =
    best.kind === "artist"
      ? t("search.kind.artist")
      : best.kind === "album"
        ? `${t("search.kind.album")} · ${best.item.artist}`
        : `${t("search.kind.track")} · ${best.item.artists}`;
  return (
    <button
      type="button"
      onClick={onOpen}
      aria-label={title}
      className="group flex flex-col gap-4 rounded-(--radius-xl) bg-bg-raised p-5 text-left hover:bg-bg-hover"
    >
      {best.kind === "artist" ? (
        <span className="t-hero flex size-24 items-center justify-center rounded-full bg-artwork-placeholder text-text-tertiary">
          {title.slice(0, 2).toUpperCase()}
        </span>
      ) : (
        <Cover
          hash={best.kind === "album" ? best.item.cover : null}
          px={96}
          rounded="md"
          className="shadow-(--shadow-artwork)"
        />
      )}
      <div className="flex items-end gap-3">
        <div className="min-w-0 flex-1">
          <div className="t-page-title truncate text-text-primary">{title}</div>
          <div className="t-caption truncate text-text-secondary">{sub}</div>
        </div>
        {best.kind === "track" && (
          <span className="flex size-12 items-center justify-center rounded-full bg-accent-fill text-accent-on">
            <Play size={20} fill="currentColor" strokeWidth={1.5} />
          </span>
        )}
      </div>
    </button>
  );
}

function PlaylistHits({
  items,
  onOpen,
}: {
  items: { id: number; name: string }[];
  onOpen: (id: number) => void;
}) {
  return (
    <ul className="m-0 flex list-none flex-col gap-1 p-0">
      {items.map((p) => (
        <li key={p.id}>
          <button
            type="button"
            onClick={() => onOpen(p.id)}
            className="flex h-(--row-height) w-full items-center gap-3 rounded-(--radius-md) px-3 text-left hover:bg-bg-hover"
          >
            <ListMusic size={20} strokeWidth={1.5} className="text-icon" />
            <span className="t-body truncate text-text-primary">{p.name}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}
