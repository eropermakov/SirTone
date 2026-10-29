import { useDroppable } from "@dnd-kit/core";
import { MoreHorizontal, Pencil, Play, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { dropId } from "@/app/dnd/dnd";
import { Mosaic } from "@/app/playlists/Mosaic";
import { playTracks } from "@/shared/ipc/library";
import {
  deletePlaylist,
  type PlaylistItem,
  playlistTracks,
  renamePlaylist,
} from "@/shared/ipc/playlists";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { Menu } from "@/shared/ui/Menu";
import { notify } from "@/shared/ui/notify";
import { PlayingBars } from "@/shared/ui/PlayingBars";

/** Hover longer than this shows the details popover (DESIGN.md Sidebar). */
const POPOVER_MS = 500;

/**
 * A playlist in the sidebar (DESIGN.md Sidebar): hover — «…» and a pencil on the right, a details
 * popover after 500 ms; the playing playlist — `accent-text` bars on the right. Dropping a track
 * adds it (Spotify, `01b` F-35) — not to a smart one (read-only, Apple).
 */
export function SidebarPlaylist({
  item,
  active,
  playing,
  onOpen,
  onDeleted,
}: {
  item: PlaylistItem;
  active: boolean;
  playing: boolean;
  onOpen: () => void;
  onDeleted: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const {
    setNodeRef,
    isOver,
    active: dragging,
  } = useDroppable({
    id: dropId.playlist(item.id),
    disabled: item.smart,
  });
  const row = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(item.name);
  const [asking, setAsking] = useState(false);
  const [popover, setPopover] = useState<{ x: number; y: number } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const input = useRef<HTMLInputElement>(null);

  const hidePopover = () => {
    window.clearTimeout(timer.current);
    setPopover(null);
  };
  useEffect(() => () => window.clearTimeout(timer.current), []);
  useEffect(() => {
    if (renaming) {
      setDraft(item.name);
      requestAnimationFrame(() => input.current?.select());
    }
  }, [renaming, item.name]);

  const save = () => {
    setRenaming(false);
    const name = draft.trim();
    if (!name || name === item.name) return;
    renamePlaylist(item.id, name).catch(() => notify(t("playlists.failed")));
  };
  const play = () =>
    void playlistTracks(item.id)
      .then((entries) => {
        const ids = entries.map((e) => e.track.id);
        if (ids.length > 0) playTracks(ids, 0, "playlist", item.id);
      })
      .catch(() => notify(t("playlists.failed")));

  const minutes = Math.round((item.durationMs ?? 0) / 60000);
  const tone = active ? "text-on-primary" : "text-icon";

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the pointer only times the details popover; the keyboard reaches the same actions through the buttons
    <div
      ref={(el) => {
        setNodeRef(el);
        row.current = el;
      }}
      onMouseEnter={() => {
        window.clearTimeout(timer.current);
        timer.current = window.setTimeout(() => {
          const r = row.current?.getBoundingClientRect();
          if (r) setPopover({ x: r.right + 8, y: r.top });
        }, POPOVER_MS);
      }}
      onMouseLeave={hidePopover}
      onMouseDown={hidePopover}
      className={cn(
        "group relative flex h-(--item-height) w-full shrink-0 items-center rounded-(--radius-md)",
        "transition-colors duration-(--dur-fast)",
        active ? "bg-primary text-on-primary" : "text-text-primary hover:bg-bg-hover",
        isOver && dragging && "bg-bg-hover ring-2 ring-accent-text",
      )}
    >
      {renaming ? (
        <input
          ref={input}
          value={draft}
          aria-label={t("playlists.name")}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={save}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") {
              e.stopPropagation();
              setRenaming(false);
            }
          }}
          className="t-body mr-2 ml-9 min-w-0 flex-1 rounded-(--radius-sm) border border-border bg-bg-layer px-2 text-text-primary"
        />
      ) : (
        <button
          type="button"
          onClick={onOpen}
          aria-current={active ? "page" : undefined}
          className="flex h-full min-w-0 flex-1 items-center rounded-(--radius-md) pr-3 pl-11 text-left group-focus-within:pr-18 group-hover:pr-18"
        >
          <span className="t-body min-w-0 flex-1 truncate">{item.name}</span>
        </button>
      )}
      {!renaming && (
        <>
          {playing && (
            <PlayingBars
              className={cn(
                "pointer-events-none absolute right-3 group-focus-within:hidden group-hover:hidden",
                active ? "text-on-primary" : "text-accent-text",
              )}
            />
          )}
          <div className="absolute right-1 hidden items-center group-focus-within:flex group-hover:flex">
            <button
              type="button"
              aria-label={t("trackMenu.more")}
              title={t("trackMenu.more")}
              onClick={(e) => {
                hidePopover();
                const r = e.currentTarget.getBoundingClientRect();
                setMenu({ x: r.left, y: r.bottom + 4 });
              }}
              className={cn(
                "flex size-8 items-center justify-center rounded-full hover:bg-bg-hover",
                tone,
              )}
            >
              <MoreHorizontal size={16} strokeWidth={1.5} />
            </button>
            <button
              type="button"
              aria-label={t("playlists.rename")}
              title={t("playlists.rename")}
              onClick={() => {
                hidePopover();
                setRenaming(true);
              }}
              className={cn(
                "flex size-8 items-center justify-center rounded-full hover:bg-bg-hover",
                tone,
              )}
            >
              <Pencil size={16} strokeWidth={1.5} />
            </button>
          </div>
        </>
      )}
      {popover && !menu && !renaming && (
        <div
          role="tooltip"
          style={{ left: popover.x, top: popover.y }}
          className="flyout-in pointer-events-none fixed z-(--z-menu) flex w-60 items-center gap-3 rounded-(--radius-lg) border border-border bg-bg-flyout p-3 shadow-(--shadow-flyout)"
        >
          <Mosaic covers={item.covers} px={56} />
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="t-body-strong truncate text-text-primary">{item.name}</span>
            <span className="t-caption text-text-secondary">
              {t(item.smart ? "smart.kind" : "playlists.kind")}
            </span>
            <span className="t-caption text-text-secondary">
              {tc("count.tracks", { count: item.tracks })} ·{" "}
              {t("playlists.minutes", { count: minutes })}
            </span>
          </div>
        </div>
      )}
      {menu && (
        <Menu
          at={menu}
          label={t("playlists.kind")}
          onClose={() => setMenu(null)}
          items={[
            { id: "play", label: t("collection.play"), icon: Play, onSelect: play },
            {
              id: "rename",
              label: t("playlists.rename"),
              icon: Pencil,
              onSelect: () => setRenaming(true),
            },
            {
              id: "delete",
              label: t("playlists.delete"),
              icon: Trash2,
              danger: true,
              onSelect: () => setAsking(true),
            },
          ]}
        />
      )}
      {asking && (
        <Dialog
          title={t("playlists.delete")}
          closeLabel={tc("errors.close")}
          onClose={() => setAsking(false)}
        >
          <p className="t-body text-text-primary">
            {t("playlists.deleteQuestion", { name: item.name })}
          </p>
          <div className="flex flex-wrap gap-3">
            <Button
              variant="secondary"
              className="text-danger"
              onClick={() => {
                setAsking(false);
                deletePlaylist(item.id)
                  .then(() => {
                    notify(t("playlists.deleted", { name: item.name }));
                    onDeleted();
                  })
                  .catch(() => notify(t("playlists.failed")));
              }}
            >
              {t("playlists.delete")}
            </Button>
            <Button variant="ghost" onClick={() => setAsking(false)}>
              {t("trackCard.cancel")}
            </Button>
          </div>
        </Dialog>
      )}
    </div>
  );
}
