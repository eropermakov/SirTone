import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { mb } from "@/app/library/TrackCard";
import {
  type DuplicatesView,
  type DupPair,
  type DupSide,
  duplicatesList,
  keepBest,
  keepBoth,
  mergeDuplicate,
} from "@/shared/ipc/duplicates";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { notify } from "@/shared/ui/notify";

/**
 * «Дубли» (T3.5/T3.21, FR-DUP-002/003; beets actions, iTunes strictness — `01b` F-09): three
 * classes — «тот же звук», «похожие (другая версия)», «точные копии — не добавлены». Per pair:
 * «Оставить оба», «Заменить старый новым», «Убрать новый»; for all «тот же звук» — keep the
 * better file. A removed file goes to the Recycle Bin; its plays, ♡ and playlists stay with the
 * kept track.
 */
export function DuplicatesDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [view, setView] = useState<DuplicatesView | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    duplicatesList()
      .then(setView)
      .catch(() => setView({ pairs: [], exact: [] }));
  }, []);
  useEffect(load, [load]);

  const act = (p: Promise<unknown>, done: string) => {
    setBusy(true);
    p.then(() => notify(done))
      .catch(() => notify(t("dups.failed")))
      .finally(() => {
        setBusy(false);
        load();
      });
  };

  const same = view?.pairs.filter((p) => p.class === "same_sound") ?? [];
  const similar = view?.pairs.filter((p) => p.class === "similar") ?? [];

  const pairRow = (p: DupPair) => (
    <li
      key={`${p.a.track.id}-${p.b.track.id}`}
      className="flex flex-col gap-3 rounded-(--radius-md) border border-border p-3"
    >
      <div className="grid grid-cols-2 gap-3">
        <SideCard side={p.a} label={t("dups.old")} />
        <SideCard side={p.b} label={t("dups.new")} />
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          variant="ghost"
          disabled={busy}
          onClick={() => act(keepBoth(p.a.track.id, p.b.track.id), t("dups.keptBoth"))}
        >
          {t("dups.keepBoth")}
        </Button>
        <Button
          variant="ghost"
          disabled={busy}
          onClick={() => act(mergeDuplicate(p.b.track.id, p.a.track.id), t("dups.replaced"))}
        >
          {t("dups.replace")}
        </Button>
        <Button
          variant="ghost"
          disabled={busy}
          onClick={() => act(mergeDuplicate(p.a.track.id, p.b.track.id), t("dups.droppedNew"))}
        >
          {t("dups.dropNew")}
        </Button>
      </div>
    </li>
  );

  return (
    <Dialog title={t("dups.title")} closeLabel={tc("errors.close")} onClose={onClose} wide>
      {view === null ? null : view.pairs.length === 0 && view.exact.length === 0 ? (
        <p className="t-body text-text-secondary">{t("dups.none")}</p>
      ) : (
        <>
          <p className="t-caption text-text-secondary">{t("dups.hint")}</p>
          {same.length > 0 && (
            <section className="flex flex-col gap-2">
              <div className="flex flex-wrap items-center gap-3">
                <h3 className="t-body-strong flex-1 text-text-primary">
                  {t("dups.sameSound", { count: same.length })}
                </h3>
                <Button
                  variant="secondary"
                  disabled={busy}
                  onClick={() => act(keepBest(), t("dups.keptBest"))}
                >
                  {t("dups.keepBest")}
                </Button>
              </div>
              <ul className="flex flex-col gap-2">{same.map(pairRow)}</ul>
            </section>
          )}
          {similar.length > 0 && (
            <section className="flex flex-col gap-2">
              <h3 className="t-body-strong text-text-primary">
                {t("dups.similar", { count: similar.length })}
              </h3>
              <ul className="flex flex-col gap-2">{similar.map(pairRow)}</ul>
            </section>
          )}
          {view.exact.length > 0 && (
            <section className="flex flex-col gap-2">
              <h3 className="t-body-strong text-text-primary">
                {t("dups.exact", { count: view.exact.length })}
              </h3>
              <ul className="flex flex-col gap-1">
                {view.exact.slice(0, 50).map((path) => (
                  <li key={path} className="t-caption truncate text-text-secondary">
                    {path}
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </Dialog>
  );
}

function SideCard({ side, label }: { side: DupSide; label: string }) {
  const { t } = useTranslation("shell");
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="t-caption text-text-secondary">
        {label}
        {side.better ? ` · ${t("dups.better")}` : ""}
      </span>
      <span className="t-body-strong truncate text-text-primary">{side.track.title}</span>
      <span className="t-caption truncate text-text-secondary">{side.track.artists}</span>
      <span className="t-caption text-text-secondary">
        {t("dups.format", {
          codec: side.codec.toUpperCase(),
          quality: side.lossless
            ? t("dups.lossless")
            : t("dups.kbps", { value: side.bitrateKbps ?? 0 }),
          size: mb(side.sizeBytes ?? 0),
          plays: side.plays,
        })}
      </span>
    </div>
  );
}
