import { type KeyboardEvent, type PointerEvent, type RefObject, useRef, useState } from "react";
import { cn } from "@/shared/lib/utils";

type Props = {
  /** 0…1 */
  value: number;
  onCommit: (value: number) => void;
  /** Live value while dragging (volume follows the finger, seek waits for release). */
  onInput?: (value: number) => void;
  label: string;
  valueText?: string;
  /** Arrow-key step, 0…1. */
  step?: number;
  disabled?: boolean;
  className?: string;
  /** Lets the owner drive the fill (e.g. smooth playback progress) without re-rendering. */
  fillRef?: RefObject<HTMLDivElement | null>;
};

/**
 * Progress/volume in the owner's reference shape (D-75, replaces the 4 px bar of DESIGN.md §9): a
 * 6 px fully rounded translucent track, `accent-text` fill, no resting thumb (a 12 px one shows
 * while dragging or keyboard-focused), 16 px hit zone; the bar thickens on hover with scaleY. The
 * fill moves by `transform: scaleX` only (docs/06 §1).
 */
export function Slider({
  value,
  onCommit,
  onInput,
  label,
  valueText,
  step = 0.05,
  disabled,
  className,
  fillRef,
}: Props) {
  const track = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<number | null>(null);
  const shown = drag ?? value;

  const at = (e: PointerEvent) => {
    const r = track.current?.getBoundingClientRect();
    if (!r || r.width === 0) return 0;
    return Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  };
  const down = (e: PointerEvent<HTMLDivElement>) => {
    if (disabled) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    const v = at(e);
    setDrag(v);
    onInput?.(v);
  };
  const move = (e: PointerEvent<HTMLDivElement>) => {
    if (drag === null) return;
    const v = at(e);
    setDrag(v);
    onInput?.(v);
  };
  const up = (e: PointerEvent<HTMLDivElement>) => {
    if (drag === null) return;
    const v = at(e);
    setDrag(null);
    onCommit(v);
  };
  const key = (e: KeyboardEvent<HTMLDivElement>) => {
    const delta =
      e.key === "ArrowRight" || e.key === "ArrowUp"
        ? step
        : e.key === "ArrowLeft" || e.key === "ArrowDown"
          ? -step
          : null;
    if (delta === null || disabled) return;
    e.preventDefault();
    onCommit(Math.min(1, Math.max(0, value + delta)));
  };

  return (
    <div
      role="slider"
      tabIndex={disabled ? -1 : 0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(shown * 100)}
      aria-valuetext={valueText}
      aria-disabled={disabled}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={() => setDrag(null)}
      onKeyDown={key}
      className={cn(
        "group relative flex h-4 cursor-pointer touch-none items-center rounded-full aria-disabled:cursor-default",
        className,
      )}
    >
      <div
        ref={track}
        className="relative h-1.5 w-full origin-center overflow-hidden rounded-full bg-text-primary/25 transition-transform group-hover:scale-y-125 group-aria-disabled:scale-y-100"
      >
        <div
          ref={fillRef}
          className="absolute inset-0 origin-left bg-accent-text"
          style={{ transform: `scaleX(${shown})` }}
        />
      </div>
      {!disabled && (
        <div
          className={cn(
            "pointer-events-none absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-accent-text opacity-0 transition-opacity group-focus-visible:opacity-100",
            drag !== null && "opacity-100",
          )}
          style={{ left: `${shown * 100}%` }}
        />
      )}
    </div>
  );
}
