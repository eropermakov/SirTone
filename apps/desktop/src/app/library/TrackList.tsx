import { useDraggable, useDroppable } from "@dnd-kit/core";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Disc3,
  FolderSearch,
  ListEnd,
  ListMinus,
  ListPlus,
  ListStart,
  MicVocal,
  MoreHorizontal,
  Pencil,
  Play,
  Radio,
  Trash2,
} from "lucide-react";
import { type ReactNode, useCallback, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type DragTrack, dropId } from "@/app/dnd/dnd";
import { useAddToPlaylist } from "@/app/playlists/useAddToPlaylist";
import { useTrackTags } from "@/app/playlists/useTrackTags";
import { timecode } from "@/app/shell/PlayerBar";
import { type PlayContext, revealTrack, type TrackItem, waveBy } from "@/shared/ipc/library";
import { queueAppend, queuePlayNext } from "@/shared/ipc/playback";
import { cn } from "@/shared/lib/utils";
import { Menu, type MenuItem } from "@/shared/ui/Menu";
import { notify } from "@/shared/ui/notify";
import { TrackCard } from "./TrackCard";

/** Fixed row height from DESIGN.md §13 `--row-height` (virtualisation needs the number). */
const ROW = 52;

/** Opens an album or an artist (track menu «К альбому» / «К исполнителю»). */
export type GoTo = (kind: "albums" | "artists", id: number, title: string) => void;

/**
 * TrackRow list (DESIGN.md §9): 52 px pill rows, number → title + artist → album → duration.
 * Click selects, double click or Enter plays the list from that row (FR-PLAY-006); hover shows
 * Play instead of the number and «…» on the right; right click, «…», Shift+F10 or the menu key
 * open the track menu (one menu everywhere, DESIGN.md §9 order). Virtualised (S-05: 60 FPS on
 * 100 000 rows).
 */
export function TrackList({
  tracks,
  playingId,
  onPlay,
  label,
  context,
  onGo,
  onSelect,
  onRemoveFromList,
  playlist,
  footer,
}: {
  tracks: TrackItem[];
  playingId: number | null;
  onPlay: (index: number) => void;
  label: string;
  context: PlayContext;
  onGo?: GoTo | undefined;
  /** Selection follows clicks (e.g. «Неизвестные» shows the assignment panel). */
  onSelect?: ((index: number) => void) | undefined;
  /** A playlist page: «Убрать из плейлиста» in the menu and on Delete (FR-PLS-004). */
  onRemoveFromList?: ((index: number) => void) | undefined;
  /** A playlist page: rows reorder by dragging (positions as stored). */
  playlist?: { id: number; positions: number[] } | undefined;
  /** Below the rows, in the same scroll (the playlist's recommendations, T3.14). */
  footer?: ReactNode;
}) {
  const { t } = useTranslation("shell");
  const scroller = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [menu, setMenu] = useState<{ index: number; at: { x: number; y: number } } | null>(null);
  const [card, setCard] = useState<{ id: number; confirmDelete: boolean } | null>(null);
  const closeCard = useCallback(() => setCard(null), []);
  const toPlaylist = useAddToPlaylist();
  const tagsMenu = useTrackTags(menu ? (tracks[menu.index]?.id ?? null) : null);
  const v = useVirtualizer({
    count: tracks.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW,
    overscan: 8,
  });

  const startBy = (kind: "track" | "artist" | "album", id: number) => {
    waveBy(kind, id).catch(() => notify(t("trackMenu.waveFailed")));
  };
  const items = (tr: TrackItem, index: number): MenuItem[] => [
    {
      id: "next",
      label: t("trackMenu.playNext"),
      icon: ListStart,
      onSelect: () => {
        queuePlayNext([tr.id], context);
        notify(t("trackMenu.queuedNext", { title: tr.title }));
      },
    },
    {
      id: "end",
      label: t("trackMenu.addToQueue"),
      icon: ListEnd,
      onSelect: () => {
        queueAppend([tr.id], context);
        notify(t("trackMenu.queued", { title: tr.title }));
      },
    },
    // Q-23 (`06` §12): seven items, the rest under «Ещё ›». The wave arrives with Stage 4.
    {
      id: "playlist",
      label: t("trackMenu.addToPlaylist"),
      icon: ListPlus,
      submenu: toPlaylist.submenu([tr.id]),
    },
    {
      id: "wave",
      label: t("trackMenu.wave"),
      icon: Radio,
      submenu: [
        {
          id: "waveTrack",
          label: t("trackMenu.waveTrack"),
          onSelect: () => startBy("track", tr.id),
        },
        {
          id: "waveArtist",
          label: t("trackMenu.waveArtist"),
          disabled: tr.artistId == null,
          onSelect: () => tr.artistId != null && startBy("artist", tr.artistId),
        },
        {
          id: "waveAlbum",
          label: t("trackMenu.waveAlbum"),
          disabled: tr.albumId == null,
          onSelect: () => tr.albumId != null && startBy("album", tr.albumId),
        },
      ],
    },
    {
      id: "goto",
      label: t("trackMenu.goTo"),
      icon: Disc3,
      disabled: !onGo || (tr.albumId == null && tr.artistId == null),
      submenu: [
        {
          id: "album",
          label: t("trackMenu.toAlbum"),
          icon: Disc3,
          disabled: tr.albumId == null,
          onSelect: () => {
            if (tr.albumId != null) onGo?.("albums", tr.albumId, tr.album ?? "");
          },
        },
        {
          id: "artist",
          label: t("trackMenu.toArtist"),
          icon: MicVocal,
          disabled: tr.artistId == null,
          onSelect: () => {
            if (tr.artistId != null)
              onGo?.("artists", tr.artistId, tr.artists.split(", ")[0] ?? "");
          },
        },
      ],
    },
    {
      id: "more",
      label: t("trackMenu.more"),
      icon: MoreHorizontal,
      submenu: [
        {
          id: "details",
          label: t("trackMenu.details"),
          icon: Pencil,
          onSelect: () => setCard({ id: tr.id, confirmDelete: false }),
        },
        tagsMenu,
        {
          id: "reveal",
          label: t("trackMenu.reveal"),
          icon: FolderSearch,
          onSelect: () => {
            revealTrack(tr.id).catch(() => {});
          },
        },
      ],
    },
    { separator: true },
    ...(onRemoveFromList
      ? [
          {
            id: "unlist",
            label: t("playlists.removeFrom"),
            icon: ListMinus,
            onSelect: () => onRemoveFromList(index),
          },
        ]
      : []),
    {
      id: "remove",
      label: t("trackMenu.remove"),
      icon: Trash2,
      danger: true,
      onSelect: () => setCard({ id: tr.id, confirmDelete: true }),
    },
  ];

  const select = (index: number) => {
    setSelected(index);
    onSelect?.(index);
  };
  const openAt = (index: number, at: { x: number; y: number }) => {
    select(index);
    setMenu({ index, at });
  };
  const menuTrack = menu ? tracks[menu.index] : undefined;

  return (
    <div ref={scroller} className="min-h-0 flex-1 overflow-auto">
      <ul
        aria-label={label}
        className="relative m-0 list-none p-0"
        style={{ height: v.getTotalSize() }}
      >
        {v.getVirtualItems().map((row) => {
          const tr = tracks[row.index];
          if (!tr) return null;
          const playing = tr.id === playingId;
          return (
            <DragRow
              key={`${row.index}:${tr.id}`}
              track={tr}
              start={row.start}
              playlist={playlist?.id}
              position={playlist?.positions[row.index]}
            >
              <button
                type="button"
                aria-current={playing ? "true" : undefined}
                aria-pressed={selected === row.index}
                onClick={() => select(row.index)}
                onDoubleClick={() => onPlay(row.index)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  openAt(row.index, { x: e.clientX, y: e.clientY });
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    onPlay(row.index);
                  } else if (e.key === "Delete" && onRemoveFromList) {
                    e.preventDefault();
                    onRemoveFromList(row.index);
                  } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
                    e.preventDefault();
                    const r = e.currentTarget.getBoundingClientRect();
                    openAt(row.index, { x: r.right - 240, y: r.bottom });
                  }
                }}
                className={cn(
                  "flex h-(--row-height) min-w-0 flex-1 items-center gap-3 rounded-(--radius-md) pr-12 pl-3 text-left",
                  "hover:bg-bg-hover",
                  selected === row.index && "bg-bg-selected",
                )}
              >
                <span
                  className={cn(
                    "t-timecode flex w-8 shrink-0 justify-end",
                    playing ? "text-accent-text" : "text-text-tertiary",
                  )}
                >
                  <span className="group-hover:hidden">{row.index + 1}</span>
                  <Play
                    size={16}
                    fill="currentColor"
                    strokeWidth={1.5}
                    aria-hidden
                    className="hidden text-text-primary group-hover:block"
                  />
                </span>
                <div className="min-w-0 flex-1">
                  <div
                    className={cn(
                      "t-body truncate",
                      playing ? "text-accent-text" : "text-text-primary",
                    )}
                  >
                    {tr.title}
                  </div>
                  <div className="t-caption truncate text-text-secondary">{tr.artists}</div>
                </div>
                <div className="t-caption hidden w-56 truncate text-text-secondary lg:block">
                  {tr.album}
                </div>
                <span className="t-timecode w-12 shrink-0 text-right text-text-tertiary">
                  {timecode(tr.durationMs)}
                </span>
              </button>
              <button
                type="button"
                aria-label={t("trackMenu.more")}
                onClick={(e) => {
                  const r = e.currentTarget.getBoundingClientRect();
                  openAt(row.index, { x: r.right - 240, y: r.bottom + 4 });
                }}
                className="absolute right-2 flex size-8 items-center justify-center rounded-full text-icon opacity-0 hover:bg-bg-hover focus-visible:opacity-100 group-hover:opacity-100"
              >
                <MoreHorizontal size={20} strokeWidth={1.5} />
              </button>
            </DragRow>
          );
        })}
      </ul>
      {footer}
      {menu && menuTrack && (
        <Menu
          items={items(menuTrack, menu.index)}
          at={menu.at}
          label={t("trackMenu.label")}
          onClose={() => setMenu(null)}
        />
      )}
      {toPlaylist.dialog}
      {card && (
        <TrackCard
          key={card.id}
          trackId={card.id}
          confirmDelete={card.confirmDelete}
          onClose={closeCard}
        />
      )}
    </div>
  );
}

/**
 * A draggable row (T2.8): the pointer lifts it after 6 px; on a playlist page it is also a drop
 * target — the dragged item lands at this position (a line marks where).
 */
function DragRow({
  track,
  start,
  playlist,
  position,
  children,
}: {
  track: TrackItem;
  start: number;
  playlist?: number | undefined;
  position?: number | undefined;
  children: ReactNode;
}) {
  const data: DragTrack = { id: track.id, title: track.title };
  if (playlist != null && position != null) data.from = { playlist, position };
  const drag = useDraggable({ id: `track:${start}:${track.id}`, data });
  const drop = useDroppable({
    id:
      playlist != null && position != null
        ? dropId.row(playlist, position)
        : `nodrop:${start}:${track.id}`,
    disabled: playlist == null || position == null,
  });
  return (
    <li
      ref={(el) => {
        drag.setNodeRef(el);
        drop.setNodeRef(el);
      }}
      {...drag.listeners}
      className={cn(
        "group absolute inset-x-0 flex items-center",
        drag.isDragging && "opacity-40",
        drop.isOver &&
          "before:absolute before:inset-x-3 before:top-0 before:h-0.5 before:rounded-full before:bg-accent-text",
      )}
      style={{ transform: `translateY(${start}px)` }}
    >
      {children}
    </li>
  );
}
