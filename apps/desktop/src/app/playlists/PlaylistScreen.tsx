import { MoreHorizontal, Play, Trash2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { mb } from "@/app/library/TrackCard";
import { type GoTo, TrackList } from "@/app/library/TrackList";
import { playTracks } from "@/shared/ipc/library";
import {
  deletePlaylist,
  onPlaylistsChanged,
  type PlaylistEntry,
  playlistTracks,
  removeFromPlaylist,
  renamePlaylist,
} from "@/shared/ipc/playlists";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { Menu } from "@/shared/ui/Menu";
import { notify } from "@/shared/ui/notify";
import { Mosaic } from "./Mosaic";
import { usePlaylists } from "./usePlaylists";

/**
 * A manual playlist (FR-PLS-001/004; `01b` F-13): mosaic, name edited in place (saved on Enter
 * or when focus leaves — the draft saves itself, DESIGN.md §1), Play, the tracks; «Убрать из
 * плейлиста» keeps the track. Reordering by dragging comes with T2.8.
 */
export function PlaylistScreen({
  id,
  version,
  playingId,
  editName,
  onGo,
  onDeleted,
}: {
  id: number;
  /** LibraryChanged counter: removed tracks leave the page. */
  version: number;
  playingId: number | null;
  /** Just created: the name field starts in edit mode. */
  editName?: boolean;
  onGo?: GoTo | undefined;
  onDeleted: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const meta = usePlaylists().find((p) => p.id === id);
  const [entries, setEntries] = useState<PlaylistEntry[]>([]);
  const [editing, setEditing] = useState(Boolean(editName));
  const [draft, setDraft] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [asking, setAsking] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  const load = useCallback(() => {
    playlistTracks(id)
      .then(setEntries)
      .catch(() => setEntries([]));
  }, [id]);

  useEffect(() => {
    void version;
    load();
  }, [load, version]);

  useEffect(() => {
    let off: (() => void) | undefined;
    void onPlaylistsChanged((changed) => {
      if (changed === id) load();
    }).then((f) => {
      off = f;
    });
    return () => off?.();
  }, [id, load]);

  useEffect(() => {
    if (editing) {
      setDraft(meta?.name ?? "");
      requestAnimationFrame(() => input.current?.select());
    }
  }, [editing, meta?.name]);

  const tracks = entries.map((e) => e.track);
  const ids = tracks.map((x) => x.id);

  const saveName = () => {
    setEditing(false);
    const name = draft.trim();
    if (!name || name === meta?.name) return;
    renamePlaylist(id, name).catch(() => notify(t("playlists.failed")));
  };

  const unlist = (index: number) => {
    const e = entries[index];
    if (!e) return;
    // Optimistic (DESIGN.md §1): the row goes now, a failure brings the list back.
    setEntries((list) => list.filter((_, n) => n !== index));
    removeFromPlaylist(id, [e.position])
      .then(() => notify(t("playlists.removed", { title: e.track.title })))
      .catch(() => {
        load();
        notify(t("playlists.failed"));
      });
  };

  const minutes = Math.round((meta?.durationMs ?? 0) / 60000);

  return (
    <section className="flex h-full min-h-0 flex-col gap-4">
      <div className="flex items-end gap-6">
        <Mosaic covers={meta?.covers ?? []} px={184} className="shadow-(--shadow-artwork)" />
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <span className="t-caption text-text-secondary">{t("playlists.kind")}</span>
          {editing ? (
            <input
              ref={input}
              value={draft}
              aria-label={t("playlists.name")}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={saveName}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveName();
                if (e.key === "Escape") {
                  e.stopPropagation();
                  setEditing(false);
                }
              }}
              className="t-page-title w-full rounded-(--radius-md) border border-border bg-bg-layer px-2 text-primary-text"
            />
          ) : (
            <button
              type="button"
              onClick={() => setEditing(true)}
              title={t("playlists.rename")}
              className="t-page-title truncate text-left text-primary-text"
            >
              {meta?.name}
            </button>
          )}
          <p className="t-caption text-text-secondary">
            {tc("count.tracks", { count: meta?.tracks ?? 0 })} ·{" "}
            {t("playlists.minutes", { count: minutes })} ·{" "}
            {t("trackCard.mb", { value: mb(meta?.bytes ?? 0) })}
          </p>
        </div>
        <Button
          variant="icon"
          label={t("trackMenu.more")}
          onClick={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            setMenu({ x: r.left, y: r.bottom + 4 });
          }}
        >
          <MoreHorizontal size={20} strokeWidth={1.5} />
        </Button>
        <button
          type="button"
          disabled={ids.length === 0}
          onClick={() => playTracks(ids, 0, "playlist")}
          className="t-body-strong flex h-(--button-height) shrink-0 items-center gap-2 rounded-full bg-accent-fill px-5 text-accent-on disabled:opacity-40"
        >
          <Play size={20} fill="currentColor" strokeWidth={1.5} />
          {t("collection.play")}
        </button>
      </div>
      {entries.length === 0 ? (
        <p className="t-body text-text-secondary">{t("playlists.empty")}</p>
      ) : (
        <TrackList
          tracks={tracks}
          playingId={playingId}
          label={t("collection.trackList")}
          onPlay={(i) => playTracks(ids, i, "playlist")}
          context="playlist"
          onGo={onGo}
          onRemoveFromList={unlist}
          playlist={{ id, positions: entries.map((e) => e.position) }}
        />
      )}
      {menu && (
        <Menu
          at={menu}
          label={t("playlists.kind")}
          onClose={() => setMenu(null)}
          items={[
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
            {t("playlists.deleteQuestion", { name: meta?.name ?? "" })}
          </p>
          <div className="flex flex-wrap gap-3">
            <Button
              variant="secondary"
              className="text-danger"
              onClick={() => {
                setAsking(false);
                deletePlaylist(id)
                  .then(() => {
                    notify(t("playlists.deleted", { name: meta?.name ?? "" }));
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
    </section>
  );
}
