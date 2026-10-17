import type { ReactNode } from "react";
import { cn } from "@/shared/lib/utils";

/**
 * Filter chip (DESIGN.md §9 Chip): 32 px pill, `menu` text; inactive = border + text-primary,
 * active = `primary` plate + `on-primary` (the main place of `primary` in content).
 */
export function Chip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "t-menu h-(--chip-height) shrink-0 rounded-full px-3",
        active
          ? "bg-primary text-on-primary"
          : "border border-border text-text-primary hover:bg-bg-hover",
      )}
    >
      {children}
    </button>
  );
}

/**
 * Badge (DESIGN.md §9): 20 px, caps. Main = `primary` + `on-primary` («Lossless», «Hi-Res»);
 * quiet = `inactive` outline + `text-secondary` («Explicit», «Локальный»).
 */
export function Badge({ quiet, children }: { quiet?: boolean; children: ReactNode }) {
  return (
    <span
      className={cn(
        "t-badge inline-flex h-5 items-center rounded-full px-2 uppercase",
        quiet ? "border border-inactive text-text-secondary" : "bg-primary text-on-primary",
      )}
    >
      {children}
    </span>
  );
}
