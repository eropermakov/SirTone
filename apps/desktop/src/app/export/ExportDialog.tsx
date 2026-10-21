import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { type AppError, commands, type ExportProgress, events } from "@/shared/ipc/bindings";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { notify } from "@/shared/ui/notify";
import { Switch } from "@/shared/ui/Switch";

/** What to export: the whole library, a playlist, an album or an artist. */
export type ExportWhat = {
  kind: "all" | "playlist" | "album" | "artist";
  id?: number;
  title: string;
};

let asked: ExportWhat | null = null;
const subs = new Set<() => void>();
const set = (w: ExportWhat | null) => {
  asked = w;
  for (const s of subs) s();
};

/** Opens the export window (menus of albums, artists, playlists; Settings). */
export const openExport = (w: ExportWhat): void => set(w);

/**
 * «Экспорт» (T5.9, FR-DATA-003, `05` §12, AT-09): a folder, «Сохранить структуру папок» and
 * «Нумеровать по порядку»; the files are copied byte for byte, the originals untouched; progress
 * and «Отменить». Mounted once in the app; nothing on screen until asked.
 */
export function ExportHost() {
  const what = useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => asked,
  );
  return what ? <ExportDialog key={`${what.kind}-${what.id}`} what={what} /> : null;
}

function ExportDialog({ what }: { what: ExportWhat }) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [dest, setDest] = useState<string | null>(null);
  const [keep, setKeep] = useState(what.kind !== "playlist");
  const [number, setNumber] = useState(what.kind === "playlist");
  const [run, setRun] = useState<ExportProgress | null>(null);
  const running = useRef(false);
  const failText = (e: AppError | null | undefined) =>
    e ? t(`export.error.${e.code}`, { defaultValue: tc("errors.generic") }) : tc("errors.generic");

  // biome-ignore lint/correctness/useExhaustiveDependencies: subscribed once per window.
  useEffect(() => {
    if (!isTauri()) return;
    let off: (() => void) | undefined;
    let alive = true;
    void events.exportProgress
      .listen((e) => {
        if (!running.current) return;
        const p = e.payload;
        setRun(p);
        if (!p.finished) return;
        running.current = false;
        if (p.error) notify(failText(p.error));
        else if (p.cancelled) notify(t("export.cancelled", { count: p.done }));
        else notify(t("export.done", { count: p.done }));
        set(null);
      })
      .then((u) => {
        if (alive) off = u;
        else u();
      });
    return () => {
      alive = false;
      off?.();
    };
  }, []);

  const start = async () => {
    if (!dest) return;
    running.current = true;
    const r = await commands.exportStart({
      kind: what.kind,
      id: what.id ?? null,
      dest,
      keepStructure: keep,
      number,
    });
    if (r.status === "error") {
      running.current = false;
      notify(failText(r.error));
      return;
    }
    setRun({
      done: 0,
      total: r.data.files,
      bytes: 0,
      totalBytes: r.data.bytes,
      skipped: 0,
      finished: false,
      cancelled: false,
      error: null,
    });
  };
  const pick = async () => {
    if (!isTauri()) return;
    const p = await commands.libraryPickFolder();
    if (p) setDest(p);
  };
  const share =
    run && (run.totalBytes ?? 0) > 0 ? Math.min(1, (run.bytes ?? 0) / (run.totalBytes ?? 1)) : 0;
  return (
    <Dialog
      title={t("export.title", { what: what.title })}
      closeLabel={tc("errors.close")}
      onClose={() => {
        if (running.current) void commands.exportCancel();
        else set(null);
      }}
    >
      <div className="flex flex-col gap-4" data-testid="export-dialog">
        <p className="t-caption text-text-secondary">{t("export.hint")}</p>
        <div className="flex items-center gap-3">
          <span
            className="t-body min-w-0 flex-1 truncate text-text-primary"
            data-testid="export-dest"
          >
            {dest ?? t("export.noFolder")}
          </span>
          <Button variant="secondary" disabled={!!run} onClick={() => void pick()}>
            {t("export.choose")}
          </Button>
        </div>
        <div className="flex items-center gap-4">
          <span className="t-body flex-1 text-text-primary">{t("export.keep")}</span>
          <Switch checked={keep} label={t("export.keep")} onChange={setKeep} />
        </div>
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <div className="t-body text-text-primary">{t("export.number")}</div>
            <div className="t-caption text-text-secondary">{t("export.numberHint")}</div>
          </div>
          <Switch checked={number} label={t("export.number")} onChange={setNumber} />
        </div>
        {run && (
          <div className="flex flex-col gap-2" data-testid="export-progress">
            <div className="h-1.5 overflow-hidden rounded-full bg-bg-layer">
              <div
                className="h-full origin-left bg-accent-text"
                style={{ transform: `scaleX(${share})` }}
              />
            </div>
            <span className="t-caption text-text-secondary">
              {t("export.progress", { done: run.done, total: run.total })}
            </span>
          </div>
        )}
        <div className="flex justify-end gap-3">
          {run ? (
            <Button variant="ghost" onClick={() => void commands.exportCancel()}>
              {t("export.cancel")}
            </Button>
          ) : (
            <Button disabled={!dest} onClick={() => void start()}>
              {t("export.start")}
            </Button>
          )}
        </div>
      </div>
    </Dialog>
  );
}
