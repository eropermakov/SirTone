import { useDroppable } from "@dnd-kit/core";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  Combine,
  FolderOutput,
  ListEnd,
  ListStart,
  MoreHorizontal,
  Play,
  Trash2,
} from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { dropId } from "@/app/dnd/dnd";
import { openExport } from "@/app/export/ExportDialog";
import { cn } from "@/shared/lib/utils";
import { Cover } from "@/shared/ui/Cover";
import { Menu, type MenuItem } from "@/shared/ui/Menu";
import { Skeleton } from "@/shared/ui/Skeleton";

/** DESIGN.md §6/§9: cards keep 184 px; the column count follows the width. */
const CARD = 184;
const GAP = 24;
/** Card + 12 px + title (body) + caption. */
const ROW_HEIGHT = CARD + 12 + 44 + GAP;

export type Card = {
  id: number;
  title: string;
  subtitle: string;
  cover: string | null;
  /** Placeholder hint without a cover (T3.20). */
  art?: number;
};

type Props = {
  kind: "albums" | "artists";
  cards: Card[] | null;
  label: string;
  onOpen: (c: Card) => void;
  onPlay: (c: Card) => void;
  onPlayNext: (c: Card) => void;
  onAppend: (c: Card) => void;
  /** Artists only: «Удалить исполнителя» (FR-DATA-002). */
  onRemove?: ((c: Card) => void) | undefined;
  /** «Объединить с…» (artists, T3.12). */
  onMerge?: ((c: Card) => void) | undefined;
};

/** Initials for an artist without a photo (DESIGN.md §9 ArtistCard). */
function initials(name: string): string {
  const words = name.split(/\s+/).filter((w) => /\p{L}|\d/u.test(w));
  return (
    words.length > 1 ? `${words[0]?.[0] ?? ""}${words[1]?.[0] ?? ""}` : name.slice(0, 2)
  ).toUpperCase();
}

/**
 * AlbumCard / ArtistCard grid (DESIGN.md §9), virtualised by rows so 100 000 albums scroll at
 * 60 FPS (P-07). Album hover: `artwork-scrim` + round Play (`accent-fill`) and «…» (`bg-flyout`
 * + border). Artist: round picture or initials, name centred, hover pill `bg-hover`.
 */
export function CardGrid({
  kind,
  cards,
  label,
  onOpen,
  onPlay,
  onPlayNext,
  onAppend,
  onRemove,
  onMerge,
}: Props) {
  const { t } = useTranslation("shell");
  const scroller = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [menu, setMenu] = useState<{ card: Card; at: { x: number; y: number } } | null>(null);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setWidth(e?.contentRect.width ?? 0));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const columns = Math.max(1, Math.floor((width + GAP) / (CARD + GAP)));
  const list = cards ?? [];
  const rows = Math.ceil(list.length / columns);
  const v = useVirtualizer({
    count: cards ? rows : 3,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 3,
  });

  const items = (c: Card): MenuItem[] => [
    { id: "play", label: t("collection.play"), icon: Play, onSelect: () => onPlay(c) },
    { id: "next", label: t("trackMenu.playNext"), icon: ListStart, onSelect: () => onPlayNext(c) },
    { id: "end", label: t("trackMenu.addToQueue"), icon: ListEnd, onSelect: () => onAppend(c) },
    { separator: true },
    {
      id: "export",
      label: t("export.menu"),
      icon: FolderOutput,
      onSelect: () =>
        openExport({ kind: kind === "albums" ? "album" : "artist", id: c.id, title: c.title }),
    },
    ...(onMerge
      ? ([
          { separator: true },
          { id: "merge", label: t("artistMerge.menu"), icon: Combine, onSelect: () => onMerge(c) },
        ] as MenuItem[])
      : []),
    ...(onRemove
      ? ([
          { separator: true },
          {
            id: "remove",
            label: t("artistRemove.menu"),
            icon: Trash2,
            danger: true,
            onSelect: () => onRemove(c),
          },
        ] as MenuItem[])
      : []),
  ];

  return (
    <div ref={scroller} className="min-h-0 flex-1 overflow-auto">
      <ul
        aria-label={label}
        className="relative m-0 list-none p-0"
        style={{ height: v.getTotalSize() }}
      >
        {v.getVirtualItems().map((row) => (
          <li
            key={row.index}
            className="absolute inset-x-0 flex"
            style={{ transform: `translateY(${row.start}px)`, gap: GAP }}
          >
            {cards === null
              ? Array.from({ length: columns }, (_, i) => (
                  // biome-ignore lint/suspicious/noArrayIndexKey: placeholders
                  <div key={i} className="flex flex-col gap-3" style={{ width: CARD }}>
                    <Skeleton
                      className={cn(
                        "size-46",
                        kind === "artists" ? "rounded-full" : "rounded-(--radius-md)",
                      )}
                    />
                    <Skeleton className="h-3 w-3/4" />
                  </div>
                ))
              : list.slice(row.index * columns, row.index * columns + columns).map((c) => (
                  <CardShell
                    key={c.id}
                    card={c}
                    artist={kind === "artists"}
                    className={cn(
                      "group relative flex shrink-0 flex-col gap-3",
                      kind === "artists" &&
                        "items-center rounded-(--radius-lg) p-2 hover:bg-bg-hover",
                    )}
                  >
                    <button
                      type="button"
                      onClick={() => onOpen(c)}
                      aria-label={c.title}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        setMenu({ card: c, at: { x: e.clientX, y: e.clientY } });
                      }}
                      className={cn(
                        "relative block overflow-hidden",
                        kind === "artists"
                          ? "size-42 rounded-full"
                          : "size-46 rounded-(--radius-md) shadow-(--shadow-artwork)",
                      )}
                    >
                      {kind === "albums" ? (
                        <Cover hash={c.cover} px={CARD} rounded="md" art={c.art} />
                      ) : c.cover ? (
                        // Until a photo source exists: their main album's cover, round (D-114).
                        <Cover hash={c.cover} px={CARD} rounded="full" />
                      ) : (
                        <span className="t-hero flex size-full items-center justify-center bg-artwork-placeholder text-text-tertiary">
                          {initials(c.title)}
                        </span>
                      )}
                      {kind === "albums" && (
                        <span
                          aria-hidden
                          className="absolute inset-0 bg-(--artwork-scrim) opacity-0 transition-opacity duration-(--dur-fast) group-hover:opacity-100"
                        />
                      )}
                    </button>
                    {kind === "albums" && (
                      <div className="absolute top-[132px] right-3 flex gap-2 opacity-0 transition-opacity duration-(--dur-fast) group-hover:opacity-100 focus-within:opacity-100">
                        <button
                          type="button"
                          aria-label={t("trackMenu.more")}
                          onClick={(e) => {
                            const r = e.currentTarget.getBoundingClientRect();
                            setMenu({ card: c, at: { x: r.left, y: r.bottom + 4 } });
                          }}
                          className="flex size-10 items-center justify-center rounded-full border border-border bg-bg-flyout text-icon"
                        >
                          <MoreHorizontal size={20} strokeWidth={1.5} />
                        </button>
                        <button
                          type="button"
                          aria-label={t("collection.play")}
                          onClick={() => onPlay(c)}
                          className="flex size-10 items-center justify-center rounded-full bg-accent-fill text-accent-on"
                        >
                          <Play size={20} fill="currentColor" strokeWidth={1.5} />
                        </button>
                      </div>
                    )}
                    <div className={cn("min-w-0 w-full", kind === "artists" && "text-center")}>
                      <div className="t-body truncate text-text-primary">{c.title}</div>
                      {c.subtitle && (
                        <div className="t-caption truncate text-text-secondary">{c.subtitle}</div>
                      )}
                    </div>
                  </CardShell>
                ))}
          </li>
        ))}
      </ul>
      {menu && (
        <Menu
          items={items(menu.card)}
          at={menu.at}
          label={menu.card.title}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
}

/** A card; an artist card takes a dropped track (assign to this artist, `01b` F-35). */
function CardShell({
  card,
  artist,
  className,
  children,
}: {
  card: Card;
  artist: boolean;
  className: string;
  children: ReactNode;
}) {
  const { setNodeRef, isOver, active } = useDroppable({
    id: artist ? dropId.artist(card.id) : `card:${card.id}`,
    disabled: !artist,
    data: { name: card.title },
  });
  return (
    <div
      ref={setNodeRef}
      className={cn(className, isOver && active && "ring-2 ring-accent-text")}
      style={{ width: CARD }}
    >
      {children}
    </div>
  );
}
