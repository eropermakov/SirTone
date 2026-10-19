import type { TFunction } from "i18next";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { mergeArtists, undoMerge } from "@/shared/ipc/artists";
import type { ArtistItem } from "@/shared/ipc/library";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { notify } from "@/shared/ui/notify";
import type { Card } from "./CardGrid";

/** Merges two artists and offers «Отменить» (FR-ART-004: undo within the session). */
export function runMerge(
  t: TFunction<"shell">,
  keep: { id: number; name: string },
  drop: { id: number; name: string },
): Promise<void> {
  return mergeArtists(keep.id, drop.id)
    .then((r) =>
      notify(
        r.failed > 0
          ? t("artistMerge.doneSome", { name: keep.name, count: r.failed })
          : t("artistMerge.done", { name: keep.name }),
        {
          label: t("artistMerge.undo"),
          onClick: () =>
            void undoMerge(r.token)
              .then(() => notify(t("artistMerge.undone")))
              .catch(() => notify(t("artistMerge.failed"))),
        },
      ),
    )
    .catch(() => notify(t("artistMerge.failed")));
}

/**
 * «Объединить с…» (T3.12, FR-ART-004; Plex/Roon merge, `01b` F-10): pick the other spelling,
 * then whose name stays. Tracks, albums, ♡ and «никогда» join; the other name becomes an alias;
 * files move to the kept artist's folder.
 */
export function ArtistMerge({
  artist,
  artists,
  onClose,
}: {
  artist: Card;
  artists: ArtistItem[];
  onClose: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [filter, setFilter] = useState("");
  const [other, setOther] = useState<ArtistItem | null>(null);
  const [busy, setBusy] = useState(false);
  const q = filter.trim().toLowerCase();
  const list = artists
    .filter((a) => a.id !== artist.id && (q === "" || a.name.toLowerCase().includes(q)))
    .slice(0, 50);
  const me = { id: artist.id, name: artist.title };
  const merge = (keep: { id: number; name: string }, drop: { id: number; name: string }) => {
    setBusy(true);
    void runMerge(t, keep, drop).finally(onClose);
  };
  return (
    <Dialog
      title={t("artistMerge.title", { name: artist.title })}
      closeLabel={tc("errors.close")}
      onClose={onClose}
    >
      {other === null ? (
        <>
          <input
            aria-label={t("artistMerge.find")}
            placeholder={t("artistMerge.find")}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            className="t-body h-(--button-height) rounded-(--radius-md) border border-border bg-bg-layer px-3 text-text-primary"
          />
          <ul
            aria-label={t("artistMerge.list")}
            className="flex max-h-80 flex-col gap-1 overflow-auto"
          >
            {list.map((a) => (
              <li key={a.id}>
                <button
                  type="button"
                  onClick={() => setOther(a)}
                  className="t-body flex w-full items-center justify-between rounded-(--radius-md) px-3 py-2 text-left text-text-primary hover:bg-bg-hover"
                >
                  <span className="truncate">{a.name}</span>
                  <span className="t-caption shrink-0 text-text-secondary">
                    {tc("count.tracks", { count: a.tracks })}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </>
      ) : (
        <>
          <p className="t-body text-text-primary">
            {t("artistMerge.question", { a: artist.title, b: other.name })}
          </p>
          <div className="flex flex-wrap gap-3">
            <Button
              variant="primary"
              disabled={busy}
              onClick={() => merge(me, { id: other.id, name: other.name })}
            >
              {t("artistMerge.keepName", { name: artist.title })}
            </Button>
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() => merge({ id: other.id, name: other.name }, me)}
            >
              {t("artistMerge.keepName", { name: other.name })}
            </Button>
            <Button variant="ghost" onClick={() => setOther(null)}>
              {t("trackCard.cancel")}
            </Button>
          </div>
        </>
      )}
    </Dialog>
  );
}
