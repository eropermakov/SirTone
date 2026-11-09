import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cn } from "@/shared/lib/utils";

type Variant = "primary" | "secondary" | "ghost" | "icon";

const VARIANTS: Record<Variant, string> = {
  // One per screen (DESIGN.md §9): accent fill, accent-on text.
  primary:
    "h-(--button-height) gap-2 rounded-full bg-accent-fill px-5 t-body-strong text-accent-on",
  secondary:
    "h-(--button-height) gap-2 rounded-(--radius-sm) border border-border px-5 t-body-strong text-text-primary hover:bg-bg-hover",
  ghost:
    "h-(--button-height) gap-2 rounded-(--radius-sm) px-4 t-body-strong text-text-primary hover:bg-bg-hover",
  icon: "size-(--button-height) justify-center rounded-full text-icon hover:bg-bg-hover",
};

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  /** Required for `icon` buttons: their only accessible name. */
  label?: string;
  children?: ReactNode;
};

/**
 * Pill button (DESIGN.md §9 Button): Primary → Secondary → Ghost → Icon; disabled = opacity 0.4;
 * press feedback is immediate (no transition on the press itself).
 */
export function Button({ variant = "secondary", label, className, children, ...rest }: Props) {
  return (
    <button
      type="button"
      aria-label={label}
      {...rest}
      className={cn(
        "relative inline-flex shrink-0 items-center before:absolute before:top-1/2 before:left-1/2 before:size-11 before:-translate-x-1/2 before:-translate-y-1/2 disabled:pointer-events-none disabled:opacity-40",
        VARIANTS[variant],
        className,
      )}
    >
      {children}
    </button>
  );
}
