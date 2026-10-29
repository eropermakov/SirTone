import { Heart, X } from "lucide-react";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useSleepStatus } from "@/shared/ipc/sleep";
import { nightSet } from "@/shared/ipc/viz";
import { cn } from "@/shared/lib/utils";
import { ForwardIcon, PlayPauseIcon, RewindIcon } from "@/shared/ui/TransportIcons";
import { WAVES } from "@/shared/viz/palette";
import { useVizEnv } from "./useVizEnv";
import { VizBackground } from "./VizBackground";

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
  player,
}: {
  trackId: number | null;
  cover: string | null;
  title: string | null;
  artists: string | null;
  onExit: () => void;
  /** «Полный экран» (owner, 2026-10-20, D-105): the same minimal screen with the track's name
   * and the transport — ♡, ⏮, ⏯, ⏭ — fading with the rest after stillness. */
  player?:
    | {
        playing: boolean;
        liked: boolean;
        onLike: () => void;
        onToggle: () => void;
        onPrev: () => void;
        onNext: () => void;
      }
    | undefined;
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

  const sleepEnded = useSleepStatus().s.phase === "ended";
  // «Титры» have bright flat fields (D-122): the title and the buttons get a dark foot under them.
  const titles = useVizEnv().prefs?.mode === "titles";

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
      <VizBackground trackId={trackId} cover={cover} scrim={false} />
      {/* After the sleep timer: a slow fade to black (FR-SLEEP-005). */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 bg-bg-base transition-opacity ease-linear"
        style={{
          opacity: sleepEnded ? 1 : 0,
          transitionDuration: `${WAVES.sleep.black_ms}ms`,
        }}
      />
      {titles && (
        <div
          aria-hidden
          className="pointer-events-none absolute inset-x-0 bottom-0 h-2/5 bg-linear-to-t from-black/80 to-transparent"
        />
      )}
      {player ? (
        <div className="absolute inset-x-0 bottom-12 flex flex-col items-center gap-6 px-8 text-center">
          <div className="flex max-w-3xl flex-col gap-1">
            <div className="t-hero line-clamp-2 text-text-primary">{title}</div>
            <div className="t-body text-text-secondary">{artists}</div>
          </div>
          <div
            className={cn(
              "flex items-center gap-4 transition-opacity duration-(--dur-base)",
              awake ? "opacity-100" : "pointer-events-none opacity-0",
            )}
          >
            <button
              type="button"
              aria-label={t("wave.like")}
              aria-pressed={player.liked}
              onClick={player.onLike}
              className={cn(
                "flex size-12 items-center justify-center rounded-full hover:bg-bg-hover",
                player.liked ? "text-accent-text" : "text-icon",
              )}
            >
              <Heart size={24} strokeWidth={1.5} fill={player.liked ? "currentColor" : "none"} />
            </button>
            <button
              type="button"
              aria-label={t("player.prev")}
              title={t("player.prev")}
              onClick={player.onPrev}
              className="flex size-12 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
            >
              <RewindIcon size={24} />
            </button>
            <button
              type="button"
              aria-label={player.playing ? t("player.pause") : t("player.play")}
              onClick={player.onToggle}
              className="flex size-16 items-center justify-center rounded-full bg-accent-fill text-accent-on transition-transform active:scale-95"
            >
              <PlayPauseIcon playing={player.playing} size={28} />
            </button>
            <button
              type="button"
              aria-label={t("player.next")}
              title={t("player.next")}
              onClick={player.onNext}
              className="flex size-12 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
            >
              <ForwardIcon size={24} />
            </button>
          </div>
        </div>
      ) : (
        <div className="absolute bottom-8 left-8 flex flex-col gap-1 text-text-tertiary">
          <div className="t-body-strong">{title}</div>
          <div className="t-caption">{artists}</div>
        </div>
      )}
      <button
        type="button"
        aria-label={player ? t("wave.fullExit") : t("wave.nightExit")}
        title={player ? t("wave.fullExit") : t("wave.nightExit")}
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
