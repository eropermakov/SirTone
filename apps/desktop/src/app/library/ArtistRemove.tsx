import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { artistRemovalInfo, type RemovalInfo, removeArtist } from "@/shared/ipc/library";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { notify } from "@/shared/ui/notify";
import { Skeleton } from "@/shared/ui/Skeleton";
import type { Card } from "./CardGrid";
import { mb } from "./TrackCard";

/**
 * FR-DATA-002: deleting an artist asks with the number of tracks and their total size, and — as
 * for one track (FR-DATA-001) — «удалить файлы с диска» (Recycle Bin) or «только из приложения».
 */
export function ArtistRemove({ artist, onClose }: { artist: Card; onClose: () => void }) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [info, setInfo] = useState<RemovalInfo | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    artistRemovalInfo(artist.id).then(setInfo).catch(onClose);
  }, [artist.id, onClose]);

  const remove = (withFile: boolean) => {
    setBusy(true);
    removeArtist(artist.id, withFile)
      .then(() =>
        notify(
          t(withFile ? "artistRemove.trashed" : "artistRemove.removed", { name: artist.title }),
        ),
      )
      .catch(() => notify(t("artistRemove.failed")))
      .finally(onClose);
  };

  return (
    <Dialog title={t("artistRemove.menu")} closeLabel={tc("errors.close")} onClose={onClose}>
      {!info ? (
        <Skeleton className="h-4 w-2/3" />
      ) : (
        <div className="flex flex-col gap-4">
          <p className="t-body text-text-primary">
            {t("artistRemove.question", {
              name: artist.title,
              count: info.tracks,
              mb: mb(info.bytes ?? 0),
            })}
          </p>
          <div className="flex flex-wrap gap-3">
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() => remove(true)}
              className="text-danger"
            >
              {t("artistRemove.withFiles")}
            </Button>
            <Button variant="secondary" disabled={busy} onClick={() => remove(false)}>
              {t("trackCard.removeOnlyApp")}
            </Button>
            <Button variant="ghost" disabled={busy} onClick={onClose}>
              {t("trackCard.cancel")}
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
