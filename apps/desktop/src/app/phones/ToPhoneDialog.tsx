import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import type { SelectionSize } from "@/shared/ipc/bindings";
import {
  errorCode,
  NO_SELECTION,
  type PhoneInfo,
  type PhoneSelection,
  phoneHasApp,
  phonesList,
  type Quality,
  selectionSize,
  sendToPhone,
} from "@/shared/ipc/phones";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { Skeleton } from "@/shared/ui/Skeleton";
import { Size } from "./PhoneWizard";

/** What goes: one group of the library (a menu's item) and its title for the window. */
export type ToPhoneWhat = {
  title: string;
  selection: Partial<PhoneSelection>;
};

let asked: ToPhoneWhat | null = null;
const subs = new Set<() => void>();
const set = (w: ToPhoneWhat | null) => {
  asked = w;
  for (const s of subs) s();
};

/** Opens «На телефон» (menus of tracks, albums, artists, genres, playlists; D-152). */
export const openToPhone = (w: ToPhoneWhat): void => set(w);

/**
 * «На телефон» (D-152, FR-XFER-006; as MediaMonkey's «Отправить на», `01d` §G): the phone with
 * Сиртон, the quality, the size, and the transfer's progress — only what the phone lacks goes.
 * Mounted once in the app; nothing on screen until asked.
 */
export function ToPhoneHost() {
  const what = useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => asked,
  );
  return what ? <ToPhoneDialog key={JSON.stringify(what.selection)} what={what} /> : null;
}

const QUALITIES: Quality[] = ["original", "economy", "max"];
const KBPS: Record<Quality, number | null> = { original: null, economy: 128, max: 64 };

type Run =
  | { k: "idle" }
  | { k: "sending"; fraction: number }
  | { k: "done"; tracks: number; already: boolean }
  | { k: "failed"; code: string };

function ToPhoneDialog({ what }: { what: ToPhoneWhat }) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const selection: PhoneSelection = { ...NO_SELECTION, ...what.selection };
  // `null` — still asking the phones whether Сиртон is on them (over Wi-Fi up to ~10 s).
  const [phones, setPhones] = useState<PhoneInfo[] | null>(null);
  const [phone, setPhone] = useState<string | null>(null);
  const [quality, setQuality] = useState<Quality>("original");
  const [size, setSize] = useState<SelectionSize | null>(null);
  const [run, setRun] = useState<Run>({ k: "idle" });
  const alive = useRef(true);

  // biome-ignore lint/correctness/useExhaustiveDependencies: asked once per window.
  useEffect(() => {
    alive.current = true;
    selectionSize(selection)
      .then((s) => alive.current && setSize(s))
      .catch(() => {});
    phonesList()
      .then(async (s) => {
        const ready = s.phones.filter((p) => p.platform === "ios" && p.state === "ready");
        const ok = await Promise.all(ready.map((p) => phoneHasApp(p.id).catch(() => false)));
        const withApp = ready.filter((_, i) => ok[i]);
        if (!alive.current) return;
        setPhones(withApp);
        setPhone(withApp[0]?.id ?? null);
      })
      .catch(() => alive.current && setPhones([]));
    return () => {
      alive.current = false;
    };
  }, []);

  const sending = run.k === "sending";
  const start = () => {
    if (!phone) return;
    setRun({ k: "sending", fraction: 0 });
    sendToPhone(
      phone,
      quality,
      { music: true, playlists: selection.playlists.length > 0, taste: false },
      (f) => alive.current && setRun({ k: "sending", fraction: f }),
      selection,
    )
      .then((d) => alive.current && setRun({ k: "done", tracks: d.tracks, already: d.already }))
      .catch((e) => alive.current && setRun({ k: "failed", code: errorCode(e) }));
  };
  const bytes =
    size === null
      ? null
      : KBPS[quality] === null
        ? (size.bytes ?? 0)
        : ((size.durationSec ?? 0) * (KBPS[quality] ?? 0) * 1000) / 8;

  return (
    <Dialog
      title={t("phones.toPhone.title", { what: what.title })}
      closeLabel={tc("errors.close")}
      onClose={() => {
        // The transfer goes on in the core; closing only hides the window.
        set(null);
      }}
    >
      <div className="flex flex-col gap-4" data-testid="to-phone-dialog">
        {phones === null ? (
          <Skeleton className="h-10 w-full rounded-(--radius-md)" />
        ) : phones.length === 0 ? (
          <p role="status" className="t-body text-text-secondary">
            {t("phones.toPhone.noPhone")}
          </p>
        ) : (
          <>
            {phones.length > 1 && (
              <fieldset aria-label={t("phones.toPhone.which")} className="flex flex-col gap-2">
                {phones.map((p) => (
                  <label key={p.id} className="flex cursor-pointer items-center gap-2">
                    <input
                      type="radio"
                      name="to-phone"
                      checked={phone === p.id}
                      disabled={sending}
                      onChange={() => setPhone(p.id)}
                      className="size-4 accent-(--accent-fill)"
                    />
                    <span className="t-body text-text-primary">
                      {p.name ?? t("phones.unnamedIphone")}
                    </span>
                  </label>
                ))}
              </fieldset>
            )}
            <fieldset aria-label={t("phones.quality.title")} className="flex flex-col gap-2">
              <span className="t-caption text-text-secondary">{t("phones.quality.title")}</span>
              <div className="flex flex-wrap gap-2">
                {QUALITIES.map((q) => (
                  <label
                    key={q}
                    className={cn(
                      "t-menu flex h-(--chip-height) cursor-pointer items-center rounded-full border px-3 has-focus-visible:outline-2 has-focus-visible:outline-focus-ring",
                      quality === q
                        ? "border-transparent bg-primary text-on-primary"
                        : "border-border text-text-primary hover:bg-bg-hover",
                    )}
                  >
                    <input
                      type="radio"
                      name="to-phone-quality"
                      value={q}
                      checked={quality === q}
                      disabled={sending}
                      onChange={() => setQuality(q)}
                      className="sr-only"
                    />
                    {t(`phones.quality.${q}`)}
                  </label>
                ))}
              </div>
            </fieldset>
            {size !== null && bytes !== null && (
              <p className="t-caption flex gap-2 text-text-secondary">
                <span>{t("phones.toPhone.size", { count: size.tracks })}</span>
                <span>
                  <Size bytes={bytes} />
                </span>
              </p>
            )}
            <p className="t-caption text-text-tertiary">{t("phones.toPhone.onlyNew")}</p>
          </>
        )}
        {run.k === "sending" && (
          <div
            role="progressbar"
            aria-label={t("phones.transferring.title")}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(run.fraction * 100)}
            className="h-1 w-full overflow-hidden rounded-full bg-bg-layer"
          >
            <div
              className="h-full origin-left bg-accent-fill transition-transform motion-reduce:transition-none"
              style={{ transform: `scaleX(${run.fraction})` }}
            />
          </div>
        )}
        {run.k === "done" && (
          <p role="status" className="t-body text-text-secondary">
            {run.already
              ? t("phones.send.already")
              : t("phones.send.doneCount", { count: run.tracks })}
          </p>
        )}
        {run.k === "failed" && (
          <p role="alert" className="t-body text-text-secondary">
            {t(`phones.errors.${run.code}`, { defaultValue: t("phones.errors.Internal") })}
          </p>
        )}
        <div className="flex justify-end gap-3">
          {run.k === "done" || (phones !== null && phones.length === 0) ? (
            <Button onClick={() => set(null)}>{t("phones.wizard.done")}</Button>
          ) : (
            <Button disabled={!phone || sending || size?.tracks === 0} onClick={start}>
              {sending ? t("phones.transferring.title") : t("phones.toPhone.start")}
            </Button>
          )}
        </div>
      </div>
    </Dialog>
  );
}
