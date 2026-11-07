import { Cable, Smartphone, Wifi } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  errorCode,
  forgetPhone,
  type PhoneInfo,
  type PhonesStatus,
  phoneAccount,
  phonesList,
  pullFromPhone,
} from "@/shared/ipc/phones";
import { Button } from "@/shared/ui/Button";
import { notify } from "@/shared/ui/notify";
import { Skeleton } from "@/shared/ui/Skeleton";
import { PhoneWizard } from "./PhoneWizard";

/** How often the hub looks for phones while the screen is open (cheap local query). */
const POLL_MS = 3000;

/**
 * Hub «Телефоны» (stage 8, `docs/mobile/README.md` §6, FR-HUB-001…004/007): one card per phone
 * with one Primary action; it opens the iPhone wizard (`PhoneWizard`: what to install, Apple ID,
 * install, Developer Mode, trust). Polling pauses while a phone is being installed.
 */
export function PhonesScreen() {
  const { t } = useTranslation("shell");
  const [status, setStatus] = useState<PhonesStatus | null>(null);
  const [picking, setPicking] = useState<Picking | null>(null);
  // Lockdown queries must not race the transfer to the phone (MR §6.4).
  const busy = useRef(false);

  useEffect(() => {
    let alive = true;
    const look = () =>
      !busy.current &&
      phonesList()
        .then((s) => alive && setStatus(s))
        .catch(() => alive && setStatus({ phones: [], appleService: true }));
    look();
    const id = window.setInterval(look, POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  return (
    <section className="flex h-full flex-col gap-6" aria-labelledby="phones-title">
      <h1 id="phones-title" className="t-page-title text-primary-text">
        {t("phones.title")}
      </h1>
      {status === null ? (
        <Skeleton className="h-28 w-full max-w-xl rounded-(--radius-lg)" />
      ) : (
        <div className="flex max-w-xl flex-col gap-4">
          {!status.appleService && (
            <Note title={t("phones.apple.title")} text={t("phones.apple.text")} />
          )}
          {status.phones.length === 0 ? (
            <Empty />
          ) : (
            status.phones.map((p) => (
              <PhoneCard
                key={p.id}
                phone={p}
                picking={picking?.id === p.id ? picking.mode : null}
                onPick={(mode) => setPicking({ id: p.id, mode })}
                onCancel={() => setPicking(null)}
                onBusy={(b) => {
                  busy.current = b;
                }}
              />
            ))
          )}
        </div>
      )}
    </section>
  );
}

type Mode = "install" | "send";
type Picking = { id: string; mode: Mode };

function Note({ title, text }: { title: string; text: string }) {
  return (
    <div role="status" className="flex flex-col gap-1 rounded-(--radius-lg) bg-bg-raised p-4">
      <span className="t-body-strong text-text-primary">{title}</span>
      <span className="t-body text-text-secondary">{text}</span>
    </div>
  );
}

function Empty() {
  const { t } = useTranslation("shell");
  return (
    <div className="flex flex-col items-center gap-4 py-12 text-center">
      <Smartphone aria-hidden size={48} strokeWidth={1.25} className="text-icon" />
      <p className="t-body max-w-sm text-text-secondary">{t("phones.empty")}</p>
    </div>
  );
}

function PhoneCard({
  phone,
  picking,
  onPick,
  onCancel,
  onBusy,
}: {
  phone: PhoneInfo;
  picking: Mode | null;
  onPick: (mode: Mode) => void;
  onCancel: () => void;
  onBusy: (busy: boolean) => void;
}) {
  const { t } = useTranslation("shell");
  const ios = phone.platform === "ios";
  const title = phone.name ?? (ios ? t("phones.unnamedIphone") : t("phones.unnamedAndroid"));
  const details = [
    phone.model,
    phone.osVersion && ios ? t("phones.ios", { v: phone.osVersion }) : null,
  ]
    .filter(Boolean)
    .join(" · ");
  const Link = phone.usb ? Cable : Wifi;
  const [confirming, setConfirming] = useState(false);
  const [until, setUntil] = useWorksUntil(phone, picking !== null);
  // A phone this PC installed: moving and taking come first, a reinstall is the exception.
  const known = until !== null;
  const days = until === null ? null : Math.floor((until - Date.now()) / 86_400_000);
  return (
    <article
      aria-label={title}
      className="flex flex-col gap-4 rounded-(--radius-lg) bg-bg-raised p-4"
    >
      <div className="flex items-center gap-3">
        <Smartphone aria-hidden size={32} strokeWidth={1.5} className="text-icon" />
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="t-card-title truncate text-text-primary">{title}</span>
          {details && <span className="t-caption text-text-secondary">{details}</span>}
          {days !== null && (
            <span className="t-caption text-text-secondary">
              {days >= 0 ? t("phones.worksFor", { count: days }) : t("phones.expired")}
            </span>
          )}
        </div>
        <Link
          aria-label={phone.usb ? t("phones.link.usb") : t("phones.link.wifi")}
          size={20}
          strokeWidth={1.5}
          className="text-icon"
        />
      </div>
      {!ios || phone.state === "notSupportedYet" ? (
        <p className="t-body text-text-secondary">{t("phones.androidSoon")}</p>
      ) : phone.state === "needsTrust" ? (
        <p role="status" className="t-body text-text-secondary">
          {t("phones.trust")}
        </p>
      ) : picking !== null ? (
        <PhoneWizard phone={phone} mode={picking} onClose={onCancel} onBusy={onBusy} />
      ) : confirming ? (
        <div className="flex flex-col gap-3">
          <p role="alert" className="t-body text-text-secondary">
            {t("phones.forget.question")}
          </p>
          <div className="flex gap-2">
            <Button
              variant="primary"
              onClick={() => {
                setConfirming(false);
                forgetPhone(phone.id)
                  .then(() => {
                    setUntil(null);
                    notify(t("phones.forget.done"));
                  })
                  .catch((e) =>
                    notify(
                      t(`phones.errors.${errorCode(e)}`, {
                        defaultValue: t("phones.errors.Internal"),
                      }),
                    ),
                  );
              }}
            >
              {t("phones.forget.confirm")}
            </Button>
            <Button variant="ghost" onClick={() => setConfirming(false)}>
              {t("phones.choose.cancel")}
            </Button>
          </div>
        </div>
      ) : known ? (
        <div className="flex flex-wrap gap-2">
          <Button variant="primary" onClick={() => onPick("send")}>
            {t("phones.send.action")}
          </Button>
          <PullAction phone={phone} onBusy={onBusy} />
          <Button variant="ghost" onClick={() => onPick("install")}>
            {t("phones.reinstall")}
          </Button>
          <Button variant="ghost" onClick={() => setConfirming(true)}>
            {t("phones.forget.action")}
          </Button>
        </div>
      ) : (
        <div>
          <Button variant="primary" onClick={() => onPick("install")}>
            {t("phones.install")}
          </Button>
        </div>
      )}
    </article>
  );
}

/**
 * «Забрать с телефона» on a known phone: progress in place of the button, the result and any
 * failure as a toast (DESIGN.md §1). The phone list stops polling while it runs.
 */
function PullAction({ phone, onBusy }: { phone: PhoneInfo; onBusy: (busy: boolean) => void }) {
  const { t } = useTranslation("shell");
  const [fraction, setFraction] = useState<number | null>(null);
  const run = () => {
    setFraction(0);
    onBusy(true);
    pullFromPhone(phone.id, setFraction)
      .then((d) => notify(t("phones.pull.done", { count: d.newTracks, plays: d.plays })))
      .catch((e) =>
        notify(t(`phones.errors.${errorCode(e)}`, { defaultValue: t("phones.errors.Internal") })),
      )
      .finally(() => {
        onBusy(false);
        setFraction(null);
      });
  };
  if (fraction === null) {
    return <Button onClick={run}>{t("phones.pull.action")}</Button>;
  }
  const percent = Math.round(fraction * 100);
  return (
    <div className="flex w-full flex-col gap-2">
      <div
        role="progressbar"
        aria-label={t("phones.pull.title")}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
        className="h-1 w-full overflow-hidden rounded-full bg-bg-layer"
      >
        <div
          className="h-full origin-left bg-accent-fill transition-transform motion-reduce:transition-none"
          style={{ transform: `scaleX(${fraction})` }}
        />
      </div>
      <span role="status" className="t-caption text-text-secondary">
        {fraction === 0 ? t("phones.pull.title") : t("phones.pull.progress", { percent })}
      </span>
    </div>
  );
}

/** When the app on a known iPhone stops opening unless renewed (S11); `null` before the hub
 * installed it. Re-read when the wizard closes (an install sets a new end). */
function useWorksUntil(
  phone: PhoneInfo,
  picking: boolean,
): [number | null, (v: number | null) => void] {
  const [until, setUntil] = useState<number | null>(null);
  useEffect(() => {
    if (phone.platform !== "ios" || picking) return;
    let alive = true;
    phoneAccount(phone.id)
      .then((a) => alive && setUntil(a.worksUntilMs ?? null))
      .catch(() => alive && setUntil(null));
    return () => {
      alive = false;
    };
  }, [phone.id, phone.platform, picking]);
  return [until, setUntil];
}
