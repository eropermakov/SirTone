import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ErrorCode, LibraryStatus, SendParts } from "@/shared/ipc/bindings";
import { getStatus } from "@/shared/ipc/library";
import {
  answerTwoFactor,
  appleLogin,
  errorCode,
  installOnPhone,
  type PhoneInfo,
  phoneAccount,
  type Quality,
  sendToPhone,
  type TwoFactorRequest,
} from "@/shared/ipc/phones";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Skeleton } from "@/shared/ui/Skeleton";

type Choice = "all" | "pick" | "app";
const CHOICES: Choice[] = ["all", "pick", "app"];
const QUALITIES: Quality[] = ["original", "economy", "max"];
const READY: ReadonlySet<Choice> = new Set(CHOICES);
type Part = keyof SendParts;
/** «Выбрать» (MR §6.1 step 5): what can go; «Настройки и тема» do not travel yet. */
const PARTS: Part[] = ["music", "playlists", "taste"];
const ALL: SendParts = { music: true, playlists: true, taste: true };
/** Opus copy rates (D-146), kbit/s — the size estimate before sending. */
const KBPS: Record<Quality, number | null> = { original: null, economy: 128, max: 64 };

/** The music's size on the phone in the chosen quality, bytes. */
function musicBytes(status: LibraryStatus, quality: Quality): number {
  const kbps = KBPS[quality];
  return kbps === null ? (status.bytes ?? 0) : (status.durationSec * kbps * 1000) / 8;
}

type Step =
  | { k: "choose" }
  | { k: "checking" }
  | { k: "noPackage" }
  | { k: "login"; error?: ErrorCode }
  | { k: "signingIn" }
  | { k: "code"; ask: TwoFactorRequest; sent: boolean }
  | { k: "install"; fraction: number }
  | { k: "transfer"; fraction: number }
  | { k: "sent" }
  | { k: "failed"; error: ErrorCode }
  | { k: "devMode" }
  | { k: "trust" }
  | { k: "done" };

const FIELD =
  "t-body h-(--button-height) rounded-(--radius-md) border border-border bg-bg-layer px-3 text-text-primary";

/**
 * The iPhone wizard inside a phone card (`docs/mobile/README.md` §6.1, steps 4–10). Each step is
 * one view with one Primary; the hub moves on by itself where it can check. `onBusy` pauses the
 * screen's phone polling while the phone is being signed and installed.
 */
export function PhoneWizard({
  phone,
  mode = "install",
  onClose,
  onBusy,
}: {
  phone: PhoneInfo;
  /** `send`: «Перенести на телефон» on a phone that has the app — no install (it would restart
   * the app's 7-day count for nothing), only what to move. */
  mode?: "install" | "send";
  onClose: () => void;
  onBusy: (busy: boolean) => void;
}) {
  const { t } = useTranslation("shell");
  const [step, setStep] = useState<Step>({ k: "choose" });
  const [choice, setChoice] = useState<Choice>(mode === "send" ? "all" : "app");
  const choices = mode === "send" ? CHOICES.filter((c) => c !== "app") : CHOICES;
  const [quality, setQuality] = useState<Quality>("original");
  const [picked, setPicked] = useState<SendParts>(ALL);
  const [library, setLibrary] = useState<LibraryStatus | null>(null);
  const parts = choice === "pick" ? picked : ALL;
  const sends = choice !== "app" && (parts.music || parts.playlists || parts.taste);
  const [appleId, setAppleId] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const alive = useRef(true);
  useEffect(() => {
    getStatus()
      .then((s) => alive.current && setLibrary(s))
      .catch(() => {});
    return () => {
      alive.current = false;
    };
  }, []);

  const install = () => {
    setStep({ k: "install", fraction: 0 });
    onBusy(true);
    installOnPhone(phone.id, (f) => alive.current && setStep({ k: "install", fraction: f }))
      .then(async () => {
        if (!alive.current) return;
        if (sends) {
          setStep({ k: "transfer", fraction: 0 });
          await sendToPhone(
            phone.id,
            quality,
            parts,
            (f) => alive.current && setStep({ k: "transfer", fraction: f }),
          );
        }
        if (alive.current) setStep({ k: "devMode" });
      })
      .catch((e) => {
        if (!alive.current) return;
        const c = errorCode(e);
        setStep(c === "AppleSignedOut" ? { k: "login", error: c } : { k: "failed", error: c });
      })
      .finally(() => onBusy(false));
  };

  const transferOnly = () => {
    setStep({ k: "transfer", fraction: 0 });
    onBusy(true);
    sendToPhone(
      phone.id,
      quality,
      parts,
      (f) => alive.current && setStep({ k: "transfer", fraction: f }),
    )
      .then(() => alive.current && setStep({ k: "sent" }))
      .catch((e) => alive.current && setStep({ k: "failed", error: errorCode(e) }))
      .finally(() => onBusy(false));
  };

  const next = () => {
    if (mode === "send") {
      transferOnly();
      return;
    }
    setStep({ k: "checking" });
    phoneAccount(phone.id)
      .then((a) => {
        if (!alive.current) return;
        if (!a.package) setStep({ k: "noPackage" });
        else if (a.signedIn) install();
        else {
          if (a.appleId) setAppleId(a.appleId);
          setStep({ k: "login" });
        }
      })
      .catch(() => alive.current && setStep({ k: "failed", error: "Internal" }));
  };

  const signIn = () => {
    const pass = password;
    // The password leaves the form at once: it is sent to the core and dropped there (D-132).
    setPassword("");
    setStep({ k: "signingIn" });
    appleLogin(phone.id, appleId, pass, (ask) => {
      if (!alive.current) return;
      setCode("");
      setStep({ k: "code", ask, sent: false });
    })
      .then(() => alive.current && install())
      .catch((e) => alive.current && setStep({ k: "login", error: errorCode(e) }));
  };

  const sendCode = () => {
    if (step.k !== "code") return;
    setStep({ ...step, sent: true });
    void answerTwoFactor({ kind: "code", code });
  };

  const title = mode === "send" ? t("phones.send.title") : t("phones.choose.title");

  return (
    <div className="flex flex-col gap-3">
      {step.k === "choose" && (
        <>
          <span className="t-body-strong text-text-primary">{title}</span>
          <fieldset aria-label={title} className="flex flex-col gap-2">
            {choices.map((c) => (
              <label
                key={c}
                className={cn(
                  "flex flex-col gap-0.5 rounded-(--radius-md) border px-4 py-3 has-focus-visible:outline-2 has-focus-visible:outline-focus-ring",
                  !READY.has(c) && "opacity-60",
                  READY.has(c) && "cursor-pointer",
                  choice === c
                    ? "border-accent-fill bg-bg-selected"
                    : "border-border hover:bg-bg-hover",
                )}
              >
                <input
                  type="radio"
                  name={`phone-choice-${phone.id}`}
                  value={c}
                  checked={choice === c}
                  disabled={!READY.has(c)}
                  onChange={() => setChoice(c)}
                  className="sr-only"
                />
                <span className="t-body-strong text-text-primary">
                  {t(`phones.choose.${c}.title`)}
                </span>
                <span className="t-caption text-text-secondary">
                  {READY.has(c) ? t(`phones.choose.${c}.text`) : t("phones.choose.later")}
                </span>
              </label>
            ))}
          </fieldset>
          {choice === "pick" && (
            <fieldset aria-label={t("phones.parts.title")} className="flex flex-col gap-2">
              {PARTS.map((part) => (
                <label
                  key={part}
                  className="flex cursor-pointer items-center gap-3 has-focus-visible:outline-2 has-focus-visible:outline-focus-ring"
                >
                  <input
                    type="checkbox"
                    checked={picked[part]}
                    onChange={(e) => setPicked({ ...picked, [part]: e.target.checked })}
                    className="size-4 accent-(--accent-fill)"
                  />
                  <span className="t-body text-text-primary">{t(`phones.parts.${part}`)}</span>
                  {part === "music" && library !== null && (
                    <span className="t-caption text-text-secondary">
                      <Size bytes={musicBytes(library, quality)} />
                    </span>
                  )}
                </label>
              ))}
              <span className="t-caption text-text-secondary">
                {t("phones.parts.settingsLater")}
              </span>
            </fieldset>
          )}
          {choice !== "app" && parts.music && (
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
                      name={`phone-quality-${phone.id}`}
                      value={q}
                      checked={quality === q}
                      onChange={() => setQuality(q)}
                      className="sr-only"
                    />
                    {t(`phones.quality.${q}`)}
                  </label>
                ))}
              </div>
            </fieldset>
          )}
          <Actions
            primary={t("phones.choose.next")}
            onPrimary={next}
            onCancel={onClose}
            disabled={choice === "pick" && !sends}
          />
        </>
      )}

      {step.k === "checking" && <Skeleton className="h-10 w-full rounded-(--radius-md)" />}

      {step.k === "noPackage" && (
        <>
          <p role="status" className="t-body text-text-secondary">
            {t("phones.errors.PhonePackageMissing")}
          </p>
          <Actions primary={t("phones.wizard.retry")} onPrimary={next} onCancel={onClose} />
        </>
      )}

      {(step.k === "login" || step.k === "signingIn") && (
        <form
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (appleId.trim() && password) signIn();
          }}
        >
          <span className="t-body-strong text-text-primary">{t("phones.login.title")}</span>
          <p className="t-body text-text-secondary">{t("phones.login.text")}</p>
          <label className="flex flex-col gap-1">
            <span className="t-caption text-text-secondary">{t("phones.login.appleId")}</span>
            <input
              type="email"
              autoComplete="username"
              value={appleId}
              onChange={(e) => setAppleId(e.target.value)}
              disabled={step.k === "signingIn"}
              className={FIELD}
            />
          </label>
          <label className="flex flex-col gap-1">
            <span className="t-caption text-text-secondary">{t("phones.login.password")}</span>
            <input
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              disabled={step.k === "signingIn"}
              className={FIELD}
            />
          </label>
          {step.k === "login" && step.error && (
            <p role="alert" className="t-caption text-danger">
              {t(`phones.errors.${step.error}`, { defaultValue: t("phones.errors.Internal") })}
            </p>
          )}
          <p className="t-caption text-text-tertiary">{t("phones.login.privacy")}</p>
          <div className="flex gap-2">
            <Button
              variant="primary"
              type="submit"
              disabled={step.k === "signingIn" || !appleId.trim() || !password}
            >
              {step.k === "signingIn" ? t("phones.login.busy") : t("phones.login.submit")}
            </Button>
            <Button variant="ghost" type="button" onClick={onClose}>
              {t("phones.choose.cancel")}
            </Button>
          </div>
        </form>
      )}

      {step.k === "code" && (
        <form
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            if (/^\d{6}$/.test(code)) sendCode();
          }}
        >
          <span className="t-body-strong text-text-primary">{t("phones.code.title")}</span>
          <p className="t-body text-text-secondary">
            {step.ask.sms ? t("phones.code.sms") : t("phones.code.devices")}
          </p>
          {step.ask.retry && (
            <p role="alert" className="t-caption text-danger">
              {t("phones.code.retry")}
            </p>
          )}
          <label className="flex w-40 flex-col gap-1">
            <span className="t-caption text-text-secondary">{t("phones.code.label")}</span>
            <input
              inputMode="numeric"
              autoComplete="one-time-code"
              maxLength={6}
              value={code}
              onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
              disabled={step.sent}
              className={cn(FIELD, "tabular-nums")}
            />
          </label>
          <div className="flex flex-wrap gap-2">
            <Button variant="primary" type="submit" disabled={step.sent || code.length !== 6}>
              {step.sent ? t("phones.login.busy") : t("phones.code.submit")}
            </Button>
            <Button
              variant="ghost"
              type="button"
              disabled={step.sent}
              onClick={() => {
                setStep({ ...step, sent: true });
                void answerTwoFactor({ kind: "resend" });
              }}
            >
              {t("phones.code.resend")}
            </Button>
            <Button
              variant="ghost"
              type="button"
              onClick={() => {
                void answerTwoFactor({ kind: "abort" });
                onClose();
              }}
            >
              {t("phones.choose.cancel")}
            </Button>
          </div>
        </form>
      )}

      {step.k === "install" && (
        <div className="flex flex-col gap-2">
          <span role="status" className="t-body-strong text-text-primary">
            {t("phones.installing.busy")}
          </span>
          <div
            role="progressbar"
            aria-label={t("phones.installing.busy")}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(step.fraction * 100)}
            className="h-1 w-full overflow-hidden rounded-full bg-bg-layer"
          >
            <div
              className="h-full origin-left bg-accent-fill transition-transform motion-reduce:transition-none"
              style={{ transform: `scaleX(${step.fraction})` }}
            />
          </div>
          <span className="t-caption text-text-secondary">
            {step.fraction < 0.5 ? t("phones.installing.signing") : t("phones.installing.sending")}
          </span>
        </div>
      )}

      {step.k === "transfer" && (
        <div className="flex flex-col gap-2">
          <span role="status" className="t-body-strong text-text-primary">
            {t("phones.transferring.title")}
          </span>
          <div
            role="progressbar"
            aria-label={t("phones.transferring.title")}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(step.fraction * 100)}
            className="h-1 w-full overflow-hidden rounded-full bg-bg-layer"
          >
            <div
              className="h-full origin-left bg-accent-fill transition-transform motion-reduce:transition-none"
              style={{ transform: `scaleX(${step.fraction})` }}
            />
          </div>
          <span className="t-caption text-text-secondary">
            {step.fraction === 0
              ? t("phones.transferring.preparing")
              : t("phones.transferring.progress", { percent: Math.round(step.fraction * 100) })}
          </span>
        </div>
      )}

      {step.k === "sent" && (
        <>
          <p role="status" className="t-body text-text-secondary">
            {t("phones.send.done")}
          </p>
          <div>
            <Button variant="primary" onClick={onClose}>
              {t("phones.wizard.done")}
            </Button>
          </div>
        </>
      )}

      {step.k === "failed" && (
        <>
          <p role="alert" className="t-body text-text-secondary">
            {t(`phones.errors.${step.error}`, { defaultValue: t("phones.errors.Internal") })}
          </p>
          <Actions primary={t("phones.wizard.retry")} onPrimary={next} onCancel={onClose} />
        </>
      )}

      {step.k === "devMode" && (
        <Steps
          title={t("phones.devMode.title")}
          items={t("phones.devMode.steps", { returnObjects: true }) as string[]}
          primary={t("phones.wizard.done")}
          onPrimary={() => setStep({ k: "trust" })}
        />
      )}

      {step.k === "trust" && (
        <Steps
          title={t("phones.allow.title")}
          items={t("phones.allow.steps", { returnObjects: true }) as string[]}
          primary={t("phones.wizard.done")}
          onPrimary={() => setStep({ k: "done" })}
        />
      )}

      {step.k === "done" && (
        <>
          <span role="status" className="t-body-strong text-text-primary">
            {t("phones.done.title")}
          </span>
          <p className="t-body text-text-secondary">{t("phones.done.text")}</p>
          <div>
            <Button variant="primary" onClick={onClose}>
              {t("phones.wizard.close")}
            </Button>
          </div>
        </>
      )}
    </div>
  );
}

/** «540 МБ» / «1.2 ГБ» (the collection's own format). */
function Size({ bytes }: { bytes: number }) {
  const { t } = useTranslation("shell");
  const gb = bytes / 1024 ** 3;
  return gb >= 1
    ? t("collection.sizeGb", { value: gb.toFixed(1) })
    : t("collection.sizeMb", { value: Math.round(bytes / 1024 ** 2) });
}

function Actions({
  primary,
  onPrimary,
  onCancel,
  disabled = false,
}: {
  primary: string;
  onPrimary: () => void;
  onCancel: () => void;
  disabled?: boolean;
}) {
  const { t } = useTranslation("shell");
  return (
    <div className="flex gap-2">
      <Button variant="primary" onClick={onPrimary} disabled={disabled}>
        {primary}
      </Button>
      <Button variant="ghost" onClick={onCancel}>
        {t("phones.choose.cancel")}
      </Button>
    </div>
  );
}

/** Steps 8–9: what to tap on the phone (pictures come with the owner's art, MR §6.1). */
function Steps({
  title,
  items,
  primary,
  onPrimary,
}: {
  title: string;
  items: string[];
  primary: string;
  onPrimary: () => void;
}) {
  return (
    <>
      <span className="t-body-strong text-text-primary">{title}</span>
      <ol className="t-body flex list-decimal flex-col gap-1 pl-5 text-text-secondary">
        {items.map((s) => (
          <li key={s}>{s}</li>
        ))}
      </ol>
      <div>
        <Button variant="primary" onClick={onPrimary}>
          {primary}
        </Button>
      </div>
    </>
  );
}
