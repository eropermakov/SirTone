import {
  ListMusic,
  Music2,
  Pause,
  Play,
  Repeat,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume2,
} from "lucide-react";
import { type RefObject, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  next,
  type PlaybackSnapshot,
  prev,
  seek,
  setVolume,
  togglePlay,
} from "@/shared/ipc/playback";
import { Slider } from "@/shared/ui/Slider";

/** m:ss / h:mm:ss timecode (format, not text). */
export function timecode(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/**
 * PlayerBar per DESIGN.md §9, prototype 0.2: play/pause, seek, volume work; shuffle, prev/next,
 * repeat and the queue need the queue (T1.8) and stay disabled — honest state, not fake buttons.
 */
export function PlayerBar({
  snap,
  receivedAt,
}: {
  snap: PlaybackSnapshot;
  receivedAt: RefObject<number>;
}) {
  const { t } = useTranslation("shell");
  const loaded = snap.state !== "empty";
  const playing = snap.state === "playing";
  const duration = snap.durationMs ?? 0;
  const fill = useRef<HTMLDivElement>(null);
  const [volume, setVolumeLocal] = useState(snap.volume ?? 1);

  useEffect(() => setVolumeLocal(snap.volume ?? 1), [snap.volume]);

  // Smooth progress between 4 Hz snapshots: extrapolate while playing, transform only.
  useEffect(() => {
    if (!playing || duration <= 0) return;
    let raf = 0;
    const tick = () => {
      const pos = snap.positionMs + (performance.now() - receivedAt.current);
      if (fill.current) {
        fill.current.style.transform = `scaleX(${Math.min(1, pos / duration)})`;
      }
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [playing, duration, snap.positionMs, receivedAt]);

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
  const PlayIcon = playing ? Pause : Play;

  return (
    <footer className="flex h-(--player-height) shrink-0 items-center gap-4 border-t border-separator bg-bg-base px-4">
      <div className="flex w-72 min-w-0 items-center gap-3">
        <div className="flex size-14 shrink-0 items-center justify-center rounded-(--radius-sm) bg-artwork-placeholder text-text-tertiary">
          <Music2 size={20} strokeWidth={1.5} />
        </div>
        <div className="min-w-0">
          <div className="t-body-strong truncate text-text-primary" title={snap.title ?? undefined}>
            {snap.title ?? t("player.nothing")}
          </div>
          <div className="t-caption truncate text-text-secondary">
            {snap.state === "ended"
              ? t("player.ended")
              : loaded
                ? (snap.artists ?? "")
                : t("player.hint")}
          </div>
        </div>
      </div>

      <div className="flex flex-1 flex-col items-center gap-1">
        <div className="flex items-center gap-2">
          {icon(Shuffle, t("player.shuffle"))}
          <button
            type="button"
            disabled={!loaded}
            onClick={prev}
            aria-label={t("player.prev")}
            className="flex size-9 items-center justify-center rounded-full text-icon disabled:opacity-40"
          >
            <SkipBack size={20} strokeWidth={1.5} />
          </button>
          <button
            type="button"
            disabled={!loaded}
            onClick={togglePlay}
            aria-label={playing ? t("player.pause") : t("player.play")}
            className="flex size-(--play-size) items-center justify-center rounded-full bg-accent-fill text-accent-on disabled:opacity-40"
          >
            <PlayIcon size={20} fill="currentColor" strokeWidth={1.5} />
          </button>
          <button
            type="button"
            disabled={!snap.hasNext}
            onClick={next}
            aria-label={t("player.next")}
            className="flex size-9 items-center justify-center rounded-full text-icon disabled:opacity-40"
          >
            <SkipForward size={20} strokeWidth={1.5} />
          </button>
          {icon(Repeat, t("player.repeat"))}
        </div>
        <div className="flex w-full max-w-xl items-center gap-2">
          <span className="t-timecode w-12 text-right text-text-tertiary">
            {timecode(snap.positionMs)}
          </span>
          <Slider
            className="flex-1"
            fillRef={fill}
            label={t("player.seek")}
            valueText={`${timecode(snap.positionMs)} / ${timecode(duration)}`}
            value={duration > 0 ? Math.min(1, snap.positionMs / duration) : 0}
            step={duration > 0 ? 5000 / duration : 0.05}
            disabled={!loaded || duration <= 0}
            onCommit={(v) => seek(v * duration)}
          />
          <span className="t-timecode w-12 text-text-tertiary">{timecode(duration)}</span>
        </div>
      </div>

      <div className="flex w-72 items-center justify-end gap-2">
        {icon(ListMusic, t("player.queue"))}
        <Volume2 size={20} strokeWidth={1.5} aria-hidden className="text-icon" />
        <Slider
          className="w-24"
          label={t("player.volume")}
          valueText={`${Math.round(volume * 100)}%`}
          value={volume}
          onInput={(v) => {
            setVolumeLocal(v);
            setVolume(v);
          }}
          onCommit={(v) => {
            setVolumeLocal(v);
            setVolume(v);
          }}
        />
      </div>
    </footer>
  );
}
