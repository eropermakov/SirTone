import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { nightSet } from "@/shared/ipc/viz";
import { cn } from "@/shared/lib/utils";
import { WAVES } from "@/shared/viz/palette";
import { WavesBackground } from "./WavesBackground";

/**
 * «Ночной вайб» (T4.7, FR-VIZ-009, `06` §7.5): the visualisation on the whole screen — the window
 * goes full screen, the display stays awake while music plays (Rust, `night.rs`). The controls
 * and the cursor fade after `night.hide_after_ms` of stillness and come back on any movement or
 * key (copy of Poweramp «Fade/Hidden» and Яндекс «Моя волна», `01a` §B.3); `Esc` leaves; the
 * track's title stays barely visible.
 */
export function NightVibe({
  trackId,
  cover,
  title,
  artists,
  onExit,
}: {
  trackId: number | null;
  cover: string | null;
  title: string | null;
  artists: string | null;
  onExit: () => void;
}) {
  const { t } = useTranslation("shell");
  const [awake, setAwake] = useState(true);

  useEffect(() => {
    void nightSet(true);
    return () => {
      void nightSet(false);
    };
  }, []);

  useEffect(() => {
    let timer = window.setTimeout(() => setAwake(false), WAVES.night.hide_after_ms);
    const wake = () => {
      setAwake(true);
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setAwake(false), WAVES.night.hide_after_ms);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onExit();
      else wake();
    };
    window.addEventListener("pointermove", wake);
    window.addEventListener("pointerdown", wake);
    window.addEventListener("keydown", key);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("pointermove", wake);
      window.removeEventListener("pointerdown", wake);
      window.removeEventListener("keydown", key);
    };
  }, [onExit]);

  return createPortal(
    <div
      data-theme="dark"
      data-viz-scope
      data-testid="night-vibe"
      className={cn(
        "fixed inset-0 z-(--z-modal) bg-bg-base text-text-primary",
        !awake && "cursor-none",
      )}
    >
      <WavesBackground trackId={trackId} cover={cover} scrim={false} />
      <div className="absolute bottom-8 left-8 flex flex-col gap-1 text-text-tertiary">
        <div className="t-body-strong">{title}</div>
        <div className="t-caption">{artists}</div>
      </div>
      <button
        type="button"
        aria-label={t("wave.nightExit")}
        title={t("wave.nightExit")}
        onClick={onExit}
        className={cn(
          "absolute top-6 right-6 flex size-12 items-center justify-center rounded-full bg-bg-flyout text-icon transition-opacity duration-(--dur-base) hover:bg-bg-hover",
          awake ? "opacity-100" : "pointer-events-none opacity-0",
        )}
      >
        <X size={24} strokeWidth={1.5} />
      </button>
    </div>,
    document.body,
  );
}
