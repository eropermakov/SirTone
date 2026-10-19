import { ListPlus, Plus } from "lucide-react";
import { type ReactNode, useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { addToPlaylist, alreadyIn, createPlaylist } from "@/shared/ipc/playlists";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import type { MenuItem } from "@/shared/ui/Menu";
import { notify } from "@/shared/ui/notify";
import { nextName, usePlaylists } from "./usePlaylists";

type Pending = { id: number; name: string; tracks: number[]; dup: number[] };

/**
 * «В плейлист ›» (DESIGN.md §9 track menu; `01b` F-13): «Новый плейлист» + every playlist. A
 * track that is already there asks «Добавить ещё раз / Не добавлять» (Spotify, Apple Music).
 * Returns the submenu builder and the question dialog to render.
 */
export function useAddToPlaylist(): {
  submenu: (tracks: number[]) => MenuItem[];
  /** Add to a known playlist (drag and drop onto the sidebar), with the same question. */
  add: (id: number, name: string, tracks: number[]) => void;
  dialog: ReactNode;
} {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const lists = usePlaylists();
  const manual = lists.filter((p) => !p.smart);
  const [pending, setPending] = useState<Pending | null>(null);
  const close = useCallback(() => setPending(null), []);

  const commit = (id: number, name: string, tracks: number[]) =>
    addToPlaylist(id, tracks)
      .then((n) => {
        if (n > 0) notify(t("playlists.added", { name, count: n }));
      })
      .catch(() => notify(t("playlists.failed")));

  const add = (id: number, name: string, tracks: number[]) =>
    alreadyIn(id, tracks)
      .then((dup) => {
        if (dup.length === 0) return commit(id, name, tracks);
        setPending({ id, name, tracks, dup });
      })
      .catch(() => notify(t("playlists.failed")));

  const submenu = (tracks: number[]): MenuItem[] => [
    {
      id: "new",
      label: t("playlists.new"),
      icon: Plus,
      onSelect: () => {
        const name = nextName(lists, (n) => t("playlists.defaultName", { n }));
        createPlaylist(name)
          .then((id) => commit(id, name, tracks))
          .catch(() => notify(t("playlists.failed")));
      },
    },
    ...(manual.length > 0 ? ([{ separator: true }] as MenuItem[]) : []),
    // Smart playlists are read-only (Apple): not offered.
    ...manual.map(
      (p): MenuItem => ({
        id: `pl-${p.id}`,
        label: p.name,
        icon: ListPlus,
        onSelect: () => void add(p.id, p.name, tracks),
      }),
    ),
  ];

  const dialog = pending && (
    <Dialog title={t("playlists.dupTitle")} closeLabel={tc("errors.close")} onClose={close}>
      <p className="t-body text-text-primary">
        {t("playlists.dupQuestion", { name: pending.name, count: pending.dup.length })}
      </p>
      <div className="flex flex-wrap gap-3">
        <Button
          variant="primary"
          onClick={() => {
            void commit(pending.id, pending.name, pending.tracks);
            close();
          }}
        >
          {t("playlists.dupAdd")}
        </Button>
        {pending.dup.length < pending.tracks.length && (
          <Button
            variant="secondary"
            onClick={() => {
              void commit(
                pending.id,
                pending.name,
                pending.tracks.filter((x) => !pending.dup.includes(x)),
              );
              close();
            }}
          >
            {t("playlists.dupSkip")}
          </Button>
        )}
        <Button variant="ghost" onClick={close}>
          {t("playlists.dupCancel")}
        </Button>
      </div>
    </Dialog>
  );

  return { submenu, add: (id, name, tracks) => void add(id, name, tracks), dialog };
}
