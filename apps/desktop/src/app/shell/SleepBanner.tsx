import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  sleepCancel,
  sleepExtend,
  sleepFading,
  sleepStatusNow,
  sleepWake,
  useSleepStatus,
} from "@/shared/ipc/sleep";
import { Button } from "@/shared/ui/Button";

const mmss = (ms: number) => {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

/**
 * The sleep timer in the window (T4.21, FR-SLEEP-001/006): while it runs — the time left and
 * «Отменить»; while fading — any press or key wakes it (the volume comes back) and «+15 мин»
 * extends it (Audible / Pocket Casts, `01c` W-08). Temporary look until the owner's design.
 */
export function SleepBanner() {
  const { t } = useTranslation("shell");
  const { s, at } = useSleepStatus();
  const [, tick] = useState(0);
  const active = s.phase !== "off" && s.phase !== "ended";

  // A local countdown between the events (once a second).
  useEffect(() => {
    if (!active) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [active]);

  // Any action in the window during the fade wakes the timer (input elsewhere — Rust sees it).
  useEffect(() => {
    const wake = () => {
      const cur = sleepStatusNow();
      if (cur.phase === "fading" || cur.phase === "final") void sleepWake();
    };
    window.addEventListener("pointerdown", wake, true);
    window.addEventListener("keydown", wake, true);
    return () => {
      window.removeEventListener("pointerdown", wake, true);
      window.removeEventListener("keydown", wake, true);
    };
  }, []);

  if (!active) return null;
  const left = s.phase === "woken" ? s.remainingMs : s.remainingMs - (performance.now() - at);
  return (
    <div className="flex justify-center bg-bg-base pt-2">
      <div
        role="status"
        data-testid="sleep-banner"
        className="flex items-center gap-3 rounded-full border border-border bg-bg-flyout px-4 py-1.5"
      >
        <span className="t-caption text-text-secondary">
          {s.kind === "tracks" && s.tracksLeft !== null
            ? t("sleep.leftTracks", { count: s.tracksLeft })
            : t("sleep.left", { time: mmss(left) })}
        </span>
        {sleepFading(s) && (
          <Button variant="secondary" onClick={() => void sleepExtend()}>
            {t("sleep.extend")}
          </Button>
        )}
        <Button variant="ghost" onClick={() => void sleepCancel()}>
          {t("sleep.cancel")}
        </Button>
      </div>
    </div>
  );
}
