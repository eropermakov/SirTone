import { createContext, use } from "react";
import { cn } from "@/shared/lib/utils";

/** Whether playback is paused — set once by the app shell, read by every PlayingBars. */
export const PlaybackPaused = createContext(false);

/**
 * «Играет» (DESIGN.md Sidebar and TrackRow): three bars in the current text colour — the caller
 * gives `text-accent-text` (or `text-on-primary` on a selected row). They stand still while paused.
 */
export function PlayingBars({ className }: { className?: string }) {
  const paused = use(PlaybackPaused);
  return (
    <span
      aria-hidden
      data-paused={paused ? "" : undefined}
      className={cn("playing-bars flex h-3.5 items-end gap-0.5", className)}
    >
      <span className="h-full w-0.75 rounded-full bg-current" />
      <span className="h-full w-0.75 rounded-full bg-current" />
      <span className="h-full w-0.75 rounded-full bg-current" />
    </span>
  );
}
