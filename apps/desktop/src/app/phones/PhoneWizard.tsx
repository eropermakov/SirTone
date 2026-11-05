import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ErrorCode } from "@/shared/ipc/bindings";
import {
  answerTwoFactor,
  appleLogin,
  errorCode,
  installOnPhone,
  type PhoneInfo,
  phoneAccount,
  type TwoFactorRequest,
} from "@/shared/ipc/phones";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Skeleton } from "@/shared/ui/Skeleton";

type Choice = "all" | "pick" | "app";
const CHOICES: Choice[] = ["all", "pick", "app"];
/** Transfer (M6) is not built yet: only the app can be installed for now. */
const READY: ReadonlySet<Choice> = new Set(["app"]);

type Step =
  | { k: "choose" }
  | { k: "checking" }
  | { k: "noPackage" }
  | { k: "login"; error?: ErrorCode }
  | { k: "signingIn" }
  | { k: "code"; ask: TwoFactorRequest; sent: boolean }
  | { k: "install"; fraction: number }
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
  onClose,
  onBusy,
}: {
  phone: PhoneInfo;
  onClose: () => void;
  onBusy: (busy: boolean) => void;
}) {
  const { t } = useTranslation("shell");
  const [step, setStep] = useState<Step>({ k: "choose" });
  const [choice, setChoice] = useState<Choice>("app");
  const [appleId, setAppleId] = useState("");
  const [password, setPassword] = useState("");
  const [code, setCode] = useState("");
  const alive = useRef(true);
  useEffect(
    () => () => {
      alive.current = false;
    },
    [],
  );

  const install = () => {
    setStep({ k: "install", fraction: 0 });
    onBusy(true);
    installOnPhone(phone.id, (f) => alive.current && setStep({ k: "install", fraction: f }))
      .then(() => alive.current && setStep({ k: "devMode" }))
      .catch((e) => {
        if (!alive.current) return;
        const c = errorCode(e);
        setStep(c === "AppleSignedOut" ? { k: "login", error: c } : { k: "failed", error: c });
      })
      .finally(() => onBusy(false));
  };

  const next = () => {
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

  return (
    <div className="flex flex-col gap-3">
      {step.k === "choose" && (
        <>
          <span className="t-body-strong text-text-primary">{t("phones.choose.title")}</span>
          <fieldset aria-label={t("phones.choose.title")} className="flex flex-col gap-2">
            {CHOICES.map((c) => (
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
          <Actions primary={t("phones.choose.next")} onPrimary={next} onCancel={onClose} />
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

function Actions({
  primary,
  onPrimary,
  onCancel,
}: {
  primary: string;
  onPrimary: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation("shell");
  return (
    <div className="flex gap-2">
      <Button variant="primary" onClick={onPrimary}>
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
