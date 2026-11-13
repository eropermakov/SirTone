import { useDroppable } from "@dnd-kit/core";
import {
  ChevronDown,
  CircleHelp,
  Disc3,
  ListFilter,
  ListMusic,
  Mic2,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  Search,
  Settings,
  Smartphone,
  Tags,
  Waves,
  X,
} from "lucide-react";
import { type ComponentProps, type ComponentType, useState } from "react";
import { useTranslation } from "react-i18next";
import { dropId } from "@/app/dnd/dnd";
import type { PlaylistItem } from "@/shared/ipc/playlists";
import { cn } from "@/shared/lib/utils";
import { BrandMark } from "@/shared/ui/BrandMark";
import { SidebarPlaylist } from "./SidebarPlaylist";

export type Screen =
  | "search"
  | "wave"
  | "artists"
  | "albums"
  | "tracks"
  | "genres"
  | "playlists"
  | "unknown"
  | "phones"
  | "settings";

type Item = { id: Screen; icon: ComponentType<{ size?: number; strokeWidth?: number }> };

/** Sidebar items — composition from the TZ (FR-SHELL-002), look from DESIGN.md §9 (Q-20). */
// «Поиск» is the first row of the menu (DESIGN.md §13 «Меню как у Apple»).
const TOP: Item[] = [
  { id: "search", icon: Search },
  { id: "wave", icon: Waves },
];
const COLLECTION: Item[] = [
  { id: "artists", icon: Mic2 },
  { id: "albums", icon: Disc3 },
  // «Все треки» (D-127): Apple Music «Песни» sits right after «Альбомы».
  { id: "tracks", icon: ListMusic },
  { id: "genres", icon: Tags },
];
const SETTINGS: Item = { id: "settings", icon: Settings };
// «Телефоны» (stage 8, FR-HUB-001): the hub for the phone apps.
const REST: Item[] = [
  { id: "unknown", icon: CircleHelp },
  { id: "phones", icon: Smartphone },
];

type Props = {
  current: Screen;
  collapsed: boolean;
  onNavigate: (s: Screen) => void;
  onToggle: () => void;
  /** Import in progress: line + 4 px bar above Settings. */
  progress?: { done: number; total: number; sourcesReleased: boolean } | null;
  onCancelImport?: () => void;
  /** The library folder cannot be reached (FR-FS-004). */
  offline?: boolean;
  /** Counters next to items (DESIGN.md §9: `accent-fill` circle with `accent-on`). */
  badges?: Partial<Record<Screen, number>>;
  /** Manual playlists: group «Плейлисты» with nested rows (DESIGN.md §9 Sidebar). */
  playlists?: PlaylistItem[];
  currentPlaylist?: number | null;
  /** The playlist that is playing — `accent-text` bars on its row (DESIGN.md Sidebar). */
  playingPlaylist?: number | null;
  onOpenPlaylist?: (id: number) => void;
  /** A playlist was deleted from its «…» menu (the screen showing it has to leave). */
  onPlaylistDeleted?: (id: number) => void;
  /** «Новый плейлист»; absent while the library is empty. */
  onNewPlaylist?: (() => void) | undefined;
  /** «Новый умный плейлист» (T3.13) — next to the group label. */
  onNewSmart?: (() => void) | undefined;
};

export function Sidebar({
  current,
  collapsed,
  onNavigate,
  onToggle,
  progress,
  onCancelImport,
  offline,
  badges,
  playlists = [],
  currentPlaylist = null,
  playingPlaylist = null,
  onOpenPlaylist,
  onPlaylistDeleted,
  onNewPlaylist,
  onNewSmart,
}: Props) {
  const [listsOpen, setListsOpen] = useState(true);
  const { t } = useTranslation("nav");
  const { t: ts } = useTranslation("shell");
  const row = (it: Item) => {
    const Icon = it.icon;
    const active = current === it.id && currentPlaylist == null;
    return (
      <SpringButton
        key={it.id}
        screen={it.id}
        onClick={() => onNavigate(it.id)}
        aria-current={active ? "page" : undefined}
        title={collapsed ? t(it.id) : undefined}
        className={cn(
          "flex h-(--nav-row-height) w-full items-center gap-3 rounded-full px-4 text-left",
          "transition-colors duration-(--dur-fast)",
          active
            ? "bg-primary text-on-primary"
            : "text-text-secondary hover:bg-bg-hover hover:text-text-primary",
          collapsed && "justify-center px-0",
        )}
      >
        <span className="relative flex">
          <Icon size={24} strokeWidth={2} />
          {collapsed && (badges?.[it.id] ?? 0) > 0 && (
            <span
              aria-hidden
              className="absolute -top-0.5 -right-0.5 size-2 rounded-full bg-accent-text"
            />
          )}
        </span>
        {!collapsed && <span className="t-nav min-w-0 flex-1 truncate">{t(it.id)}</span>}
        {!collapsed && (badges?.[it.id] ?? 0) > 0 && (
          <span className="t-badge flex h-5 min-w-5 items-center justify-center rounded-full bg-accent-fill px-1.5 text-accent-on">
            {badges?.[it.id]}
          </span>
        )}
      </SpringButton>
    );
  };

  return (
    <nav
      aria-label={t("collection")}
      className={cn(
        "glass relative z-10 flex h-full shrink-0 flex-col gap-1 rounded-(--radius-xl) p-3",
        collapsed ? "w-(--sidebar-collapsed)" : "w-(--sidebar-width)",
      )}
    >
      {/* Menu header (DESIGN.md §13): the mark 48, «Sir.kov» and the product. The owner's logo 1.0
          replaces the accent circle with the Сиртон mark (D-175). */}
      <div className={cn("mb-3 flex items-center gap-3", collapsed ? "justify-center" : "px-1")}>
        <BrandMark px={48} className="text-text-primary" />
        {!collapsed && (
          <>
            <span className="flex min-w-0 flex-1 flex-col">
              <span className="t-caption-brand truncate text-text-secondary">
                {t("common:app.brand")}
              </span>
              <span className="t-product truncate text-text-primary">{t("common:app.name")}</span>
            </span>
            <button
              type="button"
              onClick={onToggle}
              aria-label={t("collapse")}
              className="flex size-11 shrink-0 items-center justify-center"
            >
              <span className="flex h-[34px] w-[22px] items-center justify-center rounded-(--radius-sm) bg-bg-hover text-icon">
                <PanelLeftClose size={16} strokeWidth={1.5} />
              </span>
            </button>
          </>
        )}
      </div>
      {/* DESIGN.md §9: «Новый плейлист» first, then the main items. */}
      {onNewPlaylist && (
        <button
          type="button"
          onClick={onNewPlaylist}
          title={collapsed ? t("newPlaylist") : undefined}
          className={cn(
            "flex h-(--item-height) w-full items-center gap-3 rounded-(--radius-md) px-3 text-left text-text-primary hover:bg-bg-hover [&_svg]:text-icon",
            collapsed && "justify-center px-0",
          )}
        >
          <Plus size={20} strokeWidth={1.5} />
          {!collapsed && <span className="t-body min-w-0 flex-1 truncate">{t("newPlaylist")}</span>}
        </button>
      )}
      {TOP.map(row)}
      {!collapsed && (
        <div className="t-group-label mt-4 mb-1 px-3 text-text-secondary">{t("collection")}</div>
      )}
      {COLLECTION.map(row)}
      <div className="mt-4" />
      {REST.map(row)}
      {!collapsed && (onNewPlaylist || playlists.length > 0) && (
        <div className="mt-4 mb-1 flex items-center pr-1">
          <button
            type="button"
            onClick={() => setListsOpen((o) => !o)}
            aria-expanded={listsOpen}
            className="t-group-label flex flex-1 items-center gap-1 px-3 text-left text-text-secondary"
          >
            {t("playlists")}
            <ChevronDown
              size={16}
              strokeWidth={1.5}
              className={cn(
                "transition-transform duration-(--dur-fast)",
                !listsOpen && "-rotate-90",
              )}
            />
          </button>
          {onNewSmart && (
            <button
              type="button"
              onClick={onNewSmart}
              aria-label={t("newSmart")}
              title={t("newSmart")}
              className="flex size-7 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
            >
              <ListFilter size={16} strokeWidth={1.5} />
            </button>
          )}
        </div>
      )}
      {!collapsed && listsOpen && (
        <div className="flex min-h-0 flex-col gap-1 overflow-y-auto">
          {playlists.map((p) => (
            <SidebarPlaylist
              key={p.id}
              item={p}
              active={currentPlaylist === p.id}
              playing={playingPlaylist === p.id}
              onOpen={() => onOpenPlaylist?.(p.id)}
              onDeleted={() => onPlaylistDeleted?.(p.id)}
            />
          ))}
        </div>
      )}
      <div className="flex-1" />
      {offline && !collapsed && (
        <p role="status" className="t-caption mb-2 px-3 text-text-secondary">
          {ts("library.offline")}
        </p>
      )}
      {progress && !collapsed && (
        <div role="status" aria-live="polite" className="mb-2 flex flex-col gap-2 px-3">
          <div className="flex items-center gap-2">
            <span className="t-caption min-w-0 flex-1 truncate text-text-secondary">
              {ts("import.progress", { done: progress.done, total: progress.total })}
            </span>
            {onCancelImport && (
              <button
                type="button"
                onClick={onCancelImport}
                aria-label={ts("import.cancel")}
                className="flex size-6 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
              >
                <X size={16} strokeWidth={1.5} />
              </button>
            )}
          </div>
          <div className="h-1 overflow-hidden rounded-full bg-inactive">
            <div
              className="h-full origin-left bg-accent-text"
              style={{
                transform: `scaleX(${progress.total ? progress.done / progress.total : 0})`,
              }}
            />
          </div>
          {progress.sourcesReleased && (
            <span className="t-caption text-text-tertiary">{ts("import.eject")}</span>
          )}
        </div>
      )}
      {collapsed && (
        <button
          type="button"
          onClick={onToggle}
          aria-label={t("expand")}
          className="flex h-(--item-height) items-center justify-center rounded-(--radius-md) text-icon hover:bg-bg-hover"
        >
          <PanelLeftOpen size={20} strokeWidth={1.5} />
        </button>
      )}
      {row(SETTINGS)}
    </nav>
  );
}

type ButtonProps = ComponentProps<"button">;

/** A sidebar item that opens its section when a dragged track rests on it (spring-loading). */
function SpringButton({ screen, className, ...rest }: ButtonProps & { screen: Screen }) {
  const { setNodeRef, isOver, active } = useDroppable({ id: dropId.nav(screen) });
  return (
    <button
      ref={setNodeRef}
      type="button"
      {...rest}
      className={cn(className, isOver && active && "ring-2 ring-accent-text")}
    />
  );
}
