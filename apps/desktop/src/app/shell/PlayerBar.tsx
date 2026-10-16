import {
  ListMusic,
  Music2,
  Play,
  Repeat,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume2,
} from "lucide-react";
import { useTranslation } from "react-i18next";

/** Timecode for the empty state (format, not text). */
const ZERO_TIME = "0:00";

/**
 * PlayerBar per DESIGN.md §9 in its "nothing loaded" state. Prototype 0.1 has no audio engine
 * (Stage 1), so every control is disabled — honest state, not a fake player.
 */
export function PlayerBar() {
  const { t } = useTranslation("shell");
  const icon = (Icon: typeof Shuffle, label: string) => (
    <button
      type="button"
      disabled
      aria-label={label}
      className="flex size-9 items-center justify-center rounded-full text-icon disabled:opacity-40"
    >
      <Icon size={20} strokeWidth={1.5} />
    </button>
  );

  return (
    <footer className="flex h-(--player-height) shrink-0 items-center gap-4 border-t border-separator bg-bg-base px-4">
      <div className="flex w-72 min-w-0 items-center gap-3">
        <div className="flex size-14 shrink-0 items-center justify-center rounded-(--radius-sm) bg-artwork-placeholder text-text-tertiary">
          <Music2 size={20} strokeWidth={1.5} />
        </div>
        <div className="min-w-0">
          <div className="t-body-strong truncate text-text-primary">{t("player.nothing")}</div>
          <div className="t-caption truncate text-text-secondary">{t("player.hint")}</div>
        </div>
      </div>

      <div className="flex flex-1 flex-col items-center gap-1">
        <div className="flex items-center gap-2">
          {icon(Shuffle, t("player.shuffle"))}
          {icon(SkipBack, t("player.prev"))}
          <button
            type="button"
            disabled
            aria-label={t("player.play")}
            className="flex size-(--play-size) items-center justify-center rounded-full bg-accent-fill text-accent-on disabled:opacity-40"
          >
            <Play size={20} fill="currentColor" strokeWidth={1.5} />
          </button>
          {icon(SkipForward, t("player.next"))}
          {icon(Repeat, t("player.repeat"))}
        </div>
        <div className="flex w-full max-w-xl items-center gap-2">
          <span className="t-timecode w-10 text-right text-text-tertiary">{ZERO_TIME}</span>
          <div className="h-1 flex-1 rounded-full bg-inactive" />
          <span className="t-timecode w-10 text-text-tertiary">{ZERO_TIME}</span>
        </div>
      </div>

      <div className="flex w-72 items-center justify-end gap-2">
        {icon(ListMusic, t("player.queue"))}
        {icon(Volume2, t("player.volume"))}
        <div className="h-1 w-24 rounded-full bg-inactive" />
      </div>
    </footer>
  );
}
