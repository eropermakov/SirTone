import { ListMusic, Repeat, Repeat1, Shuffle, Volume, Volume2 } from "lucide-react";
import { type RefObject, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  next,
  type PlaybackSnapshot,
  prev,
  type RepeatMode,
  seek,
  setRepeat,
  setShuffle,
  setVolume,
  togglePlay,
} from "@/shared/ipc/playback";
import { cn } from "@/shared/lib/utils";
import { Cover } from "@/shared/ui/Cover";
import { Slider } from "@/shared/ui/Slider";
import { ForwardIcon, PlayPauseIcon, RewindIcon } from "@/shared/ui/TransportIcons";
import { BluetoothButton } from "./BluetoothButton";

/** m:ss / h:mm:ss timecode (format, not text). */
export function timecode(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** Time left, with a true minus sign: «−3:43». */
export const remaining = (positionMs: number, durationMs: number): string =>
  `−${timecode(Math.max(0, durationMs - positionMs))}`;

const NEXT_REPEAT: Record<RepeatMode, RepeatMode> = { off: "all", all: "one", one: "off" };

/**
 * PlayerBar per DESIGN.md §9: transport, seek, volume; shuffle and repeat (FR-PLAY-007) — when on,
 * the icon is `accent-text` with a 4 px dot; repeat cycles off → all → one. The queue button opens
 * the queue panel.
 */
export function PlayerBar({
  snap,
  receivedAt,
  queueOpen,
  onQueue,
}: {
  snap: PlaybackSnapshot;
  receivedAt: RefObject<number>;
  queueOpen: boolean;
  onQueue: () => void;
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

  const toggle = (
    Icon: typeof Shuffle,
    label: string,
    on: boolean,
    onClick: () => void,
    disabled = false,
  ) => (
    <button
      type="button"
      aria-label={label}
      aria-pressed={on}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "relative flex size-9 items-center justify-center rounded-full hover:bg-bg-hover disabled:opacity-40",
        on ? "text-accent-text" : "text-icon",
      )}
    >
      <Icon size={20} strokeWidth={1.5} />
      {on && (
        <span aria-hidden className="absolute bottom-0.5 size-1 rounded-full bg-accent-text" />
      )}
    </button>
  );
  const repeatLabel =
    snap.repeat === "one"
      ? t("player.repeatOne")
      : snap.repeat === "all"
        ? t("player.repeatAll")
        : t("player.repeat");

  return (
    <footer className="flex h-(--player-height) shrink-0 items-center gap-4 border-t border-separator bg-bg-base px-4">
      <div className="flex w-72 min-w-0 items-center gap-3">
        <Cover hash={snap.cover} px={56} art={snap.title ? snap.art : undefined} />
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
          {toggle(
            Shuffle,
            t("player.shuffle"),
            snap.shuffle,
            () => setShuffle(!snap.shuffle),
            !loaded,
          )}
          <button
            type="button"
            disabled={!loaded}
            onClick={prev}
            aria-label={t("player.prev")}
            className="flex size-10 items-center justify-center rounded-full text-text-primary transition-transform active:scale-90 disabled:opacity-40"
          >
            <RewindIcon size={28} />
          </button>
          <button
            type="button"
            disabled={!loaded}
            onClick={togglePlay}
            aria-label={playing ? t("player.pause") : t("player.play")}
            className="flex size-(--play-size) items-center justify-center rounded-full text-text-primary transition-transform active:scale-90 disabled:opacity-40"
          >
            <PlayPauseIcon playing={playing} size={32} />
          </button>
          <button
            type="button"
            disabled={!snap.hasNext}
            onClick={next}
            aria-label={t("player.next")}
            className="flex size-10 items-center justify-center rounded-full text-text-primary transition-transform active:scale-90 disabled:opacity-40"
          >
            <ForwardIcon size={28} />
          </button>
          {toggle(
            snap.repeat === "one" ? Repeat1 : Repeat,
            repeatLabel,
            snap.repeat !== "off",
            () => setRepeat(NEXT_REPEAT[snap.repeat]),
          )}
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
          <span className="t-timecode w-12 text-text-tertiary">
            {remaining(snap.positionMs, duration)}
          </span>
        </div>
      </div>

      <div className="flex w-72 items-center justify-end gap-2">
        <BluetoothButton />
        {toggle(ListMusic, t("player.queue"), queueOpen, onQueue)}
        <Volume size={20} strokeWidth={1.5} fill="currentColor" aria-hidden className="text-icon" />
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
        <Volume2
          size={20}
          strokeWidth={1.5}
          fill="currentColor"
          aria-hidden
          className="text-icon"
        />
      </div>
    </footer>
  );
}
