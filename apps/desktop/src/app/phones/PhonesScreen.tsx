import { Cable, Smartphone, Wifi } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { type PhoneInfo, type PhonesStatus, phonesList } from "@/shared/ipc/phones";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Skeleton } from "@/shared/ui/Skeleton";

/** How often the hub looks for phones while the screen is open (cheap local query). */
const POLL_MS = 3000;

type Choice = "all" | "pick" | "app";
const CHOICES: Choice[] = ["all", "pick", "app"];

/**
 * Hub «Телефоны» (stage 8, `docs/mobile/README.md` §6, FR-HUB-001/002/007): one card per phone
 * with one Primary action; the wizard's first steps (Apple component, trust, «Что установить?»).
 * Signing and install come with M2 after check П4 — the choice is shown, the install says so.
 */
export function PhonesScreen() {
  const { t } = useTranslation("shell");
  const [status, setStatus] = useState<PhonesStatus | null>(null);
  const [picking, setPicking] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const look = () =>
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
                picking={picking === p.id}
                onInstall={() => setPicking(p.id)}
                onCancel={() => setPicking(null)}
              />
            ))
          )}
        </div>
      )}
    </section>
  );
}

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
  onInstall,
  onCancel,
}: {
  phone: PhoneInfo;
  picking: boolean;
  onInstall: () => void;
  onCancel: () => void;
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
      ) : picking ? (
        <Choose onCancel={onCancel} />
      ) : (
        <div>
          <Button variant="primary" onClick={onInstall}>
            {t("phones.install")}
          </Button>
        </div>
      )}
    </article>
  );
}

/** Wizard step 4 «Что установить?» (MR §6.1). Install itself arrives with M2. */
function Choose({ onCancel }: { onCancel: () => void }) {
  const { t } = useTranslation("shell");
  const [choice, setChoice] = useState<Choice>("all");
  return (
    <div className="flex flex-col gap-3">
      <span className="t-body-strong text-text-primary">{t("phones.choose.title")}</span>
      <fieldset aria-label={t("phones.choose.title")} className="flex flex-col gap-2">
        {CHOICES.map((c) => (
          <label
            key={c}
            className={cn(
              "flex cursor-pointer flex-col gap-0.5 rounded-(--radius-md) border px-4 py-3 has-focus-visible:outline-2 has-focus-visible:outline-focus-ring",
              choice === c
                ? "border-accent-fill bg-bg-selected"
                : "border-border hover:bg-bg-hover",
            )}
          >
            <input
              type="radio"
              name="phone-choice"
              value={c}
              checked={choice === c}
              onChange={() => setChoice(c)}
              className="sr-only"
            />
            <span className="t-body-strong text-text-primary">{t(`phones.choose.${c}.title`)}</span>
            <span className="t-caption text-text-secondary">{t(`phones.choose.${c}.text`)}</span>
          </label>
        ))}
      </fieldset>
      <p role="status" className="t-caption text-text-secondary">
        {t("phones.choose.soon")}
      </p>
      <div className="flex gap-2">
        <Button variant="primary" disabled>
          {t("phones.choose.next")}
        </Button>
        <Button variant="ghost" onClick={onCancel}>
          {t("phones.choose.cancel")}
        </Button>
      </div>
    </div>
  );
}
