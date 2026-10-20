import { Pause, Play, SkipForward, Square } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { focusPause, focusSkip, focusStop, useFocusStatus } from "@/shared/ipc/focus";

const mmss = (ms: number) => {
  const s = Math.max(0, Math.ceil(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
};

/** The ring's geometry (a 20 px icon-size ring, `DESIGN.md` §9 icon grid). */
const R = 8;
const C = 2 * Math.PI * R;

/**
 * Focus in the window (T4.24, FR-FOCUS-002, copy of Windows Clock «Сеансы фокусировки»): a timer
 * ring with the phase and the time left, pause, skip the segment, stop, and today's count.
 * Temporary look until the owner's design.
 */
export function FocusBanner() {
  const { t } = useTranslation("shell");
  const { s, at } = useFocusStatus();
  const [, tick] = useState(0);
  const on = s.phase !== "off";
  useEffect(() => {
    if (!on || s.paused) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [on, s.paused]);
  if (!on) return null;
  const left = s.paused ? s.remainingMs : s.remainingMs - (performance.now() - at);
  const done = s.segmentMs > 0 ? 1 - Math.max(0, left) / s.segmentMs : 0;
  const icon = "flex size-8 items-center justify-center rounded-full text-icon hover:bg-bg-hover";
  return (
    <div className="flex justify-center bg-bg-base pt-2">
      <div
        role="status"
        data-testid="focus-banner"
        className="flex items-center gap-3 rounded-full border border-border bg-bg-flyout py-1 pr-2 pl-3"
      >
        <svg aria-hidden width="20" height="20" viewBox="0 0 20 20" className="-rotate-90">
          <circle cx="10" cy="10" r={R} fill="none" strokeWidth="2" className="stroke-inactive" />
          <circle
            cx="10"
            cy="10"
            r={R}
            fill="none"
            strokeWidth="2"
            strokeLinecap="round"
            className="stroke-accent-text"
            strokeDasharray={C}
            strokeDashoffset={C * (1 - done)}
          />
        </svg>
        <span className="t-caption text-text-secondary">
          {t(`focus.phase.${s.phase}`)} · {mmss(left)} · {t("focus.today", { count: s.today })}
        </span>
        <button
          type="button"
          className={icon}
          aria-label={s.paused ? t("focus.resume") : t("focus.pause")}
          title={s.paused ? t("focus.resume") : t("focus.pause")}
          onClick={() => void focusPause(!s.paused)}
        >
          {s.paused ? <Play size={16} strokeWidth={1.5} /> : <Pause size={16} strokeWidth={1.5} />}
        </button>
        <button
          type="button"
          className={icon}
          aria-label={t("focus.skip")}
          title={t("focus.skip")}
          onClick={() => void focusSkip()}
        >
          <SkipForward size={16} strokeWidth={1.5} />
        </button>
        <button
          type="button"
          className={icon}
          aria-label={t("focus.stop")}
          title={t("focus.stop")}
          onClick={() => void focusStop()}
        >
          <Square size={16} strokeWidth={1.5} />
        </button>
      </div>
    </div>
  );
}
