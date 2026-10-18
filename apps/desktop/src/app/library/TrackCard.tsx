import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { timecode } from "@/app/shell/PlayerBar";
import { removeTrack, type TrackDetails, trackDetails } from "@/shared/ipc/library";
import { Button } from "@/shared/ui/Button";
import { Badge } from "@/shared/ui/Chip";
import { Dialog } from "@/shared/ui/Dialog";
import { notify } from "@/shared/ui/notify";
import { Skeleton } from "@/shared/ui/Skeleton";
import { TrackForm } from "./TrackForm";

/** Megabytes with one decimal, Russian separator («0,6»). */
export const mb = (b: number) =>
  (b / 1024 / 1024).toLocaleString("ru-RU", { minimumFractionDigits: 1, maximumFractionDigits: 1 });

/**
 * Track card (FR-LIB-006/007, FR-DATA-001): format, bitrate, size, path; edit (the file moves to
 * its canonical place); delete — always asks «удалить файл с диска» (Recycle Bin) or «только
 * убрать из приложения».
 */
export function TrackCard({
  trackId,
  confirmDelete,
  onClose,
}: {
  trackId: number;
  /** Opened from «Удалить из медиатеки»: straight to the question. */
  confirmDelete?: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [d, setD] = useState<TrackDetails | null>(null);
  const [asking, setAsking] = useState(Boolean(confirmDelete));
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    trackDetails(trackId).then(setD).catch(onClose);
  }, [trackId, onClose]);

  const remove = (withFile: boolean) => {
    if (!d) return;
    setBusy(true);
    removeTrack(d.id, withFile)
      .then(() => {
        notify(t(withFile ? "trackCard.trashed" : "trackCard.removed", { title: d.title }));
        onClose();
      })
      .catch(() => {
        setBusy(false);
        notify(t("trackCard.removeFailed"));
      });
  };

  const kbps = d && d.durationMs > 0 ? Math.round(((d.sizeBytes ?? 0) * 8) / d.durationMs) : 0;

  return (
    <Dialog
      title={d?.title ?? t("trackCard.title")}
      closeLabel={tc("errors.close")}
      onClose={onClose}
    >
      {!d ? (
        <div className="flex flex-col gap-3">
          <Skeleton className="h-4 w-2/3" />
          <Skeleton className="h-4 w-1/2" />
          <Skeleton className="h-24 w-full" />
        </div>
      ) : asking ? (
        <div className="flex flex-col gap-4">
          <p className="t-body text-text-primary">
            {t("trackCard.removeQuestion", { title: d.title })}
          </p>
          <div className="flex flex-wrap gap-3">
            <Button
              variant="secondary"
              disabled={busy}
              onClick={() => remove(true)}
              className="text-danger"
            >
              {t("trackCard.removeWithFile")}
            </Button>
            <Button variant="secondary" disabled={busy} onClick={() => remove(false)}>
              {t("trackCard.removeOnlyApp")}
            </Button>
            <Button
              variant="ghost"
              disabled={busy}
              onClick={confirmDelete ? onClose : () => setAsking(false)}
            >
              {t("trackCard.cancel")}
            </Button>
          </div>
        </div>
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-2">
            {d.lossless && <Badge>{t("trackCard.lossless")}</Badge>}
            {(d.sampleRate ?? 0) > 48000 && <Badge>{t("trackCard.hiRes")}</Badge>}
            <Badge quiet>{d.codec.toUpperCase()}</Badge>
          </div>
          <dl className="t-caption grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-text-secondary">
            <dt>{t("trackCard.format")}</dt>
            <dd className="text-text-primary">
              {[
                d.container.toUpperCase(),
                d.sampleRate
                  ? t("trackCard.khz", {
                      value: (d.sampleRate / 1000).toLocaleString("ru-RU", {
                        maximumFractionDigits: 1,
                      }),
                    })
                  : null,
                d.channels ? t("trackCard.channels", { count: d.channels }) : null,
                kbps ? t("trackCard.kbps", { value: kbps }) : null,
              ]
                .filter(Boolean)
                .join(" · ")}
            </dd>
            <dt>{t("trackCard.duration")}</dt>
            <dd className="text-text-primary">{timecode(d.durationMs)}</dd>
            <dt>{t("trackCard.size")}</dt>
            <dd className="text-text-primary">
              {t("trackCard.mb", { value: mb(d.sizeBytes ?? 0) })}
            </dd>
            <dt>{t("trackCard.path")}</dt>
            <dd className="break-all text-text-primary">{d.path}</dd>
          </dl>
          <hr className="h-px border-0 bg-separator" />
          <TrackForm
            trackId={d.id}
            initial={{
              title: d.title,
              artists: d.artists,
              album: d.album ?? "",
              trackNo: d.trackNo != null ? String(d.trackNo) : "",
              year: d.year != null ? String(d.year) : "",
            }}
            submitLabel={t("trackCard.save")}
            onDone={(title) => {
              notify(t("trackCard.saved", { title }));
              onClose();
            }}
          />
          <hr className="h-px border-0 bg-separator" />
          <Button
            variant="ghost"
            onClick={() => setAsking(true)}
            className="self-start text-danger"
          >
            {t("trackMenu.remove")}
          </Button>
        </>
      )}
    </Dialog>
  );
}
