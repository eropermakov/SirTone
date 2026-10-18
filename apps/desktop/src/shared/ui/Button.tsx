import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cn } from "@/shared/lib/utils";

type Variant = "primary" | "secondary" | "ghost" | "icon";

const VARIANTS: Record<Variant, string> = {
  // One per screen (DESIGN.md §9): accent fill, accent-on text.
  primary: "h-(--button-height) gap-2 bg-accent-fill px-5 t-body-strong text-accent-on",
  secondary:
    "h-(--button-height) gap-2 border border-border px-5 t-body-strong text-text-primary hover:bg-bg-hover",
  ghost: "h-(--button-height) gap-2 px-4 t-body-strong text-text-primary hover:bg-bg-hover",
  icon: "size-(--button-height) justify-center text-icon hover:bg-bg-hover",
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
        "inline-flex shrink-0 items-center rounded-full disabled:pointer-events-none disabled:opacity-40",
        VARIANTS[variant],
        className,
      )}
    >
      {children}
    </button>
  );
}
