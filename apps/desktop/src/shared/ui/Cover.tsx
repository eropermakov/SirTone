import { Music2 } from "lucide-react";
import { useState } from "react";
import { cn } from "@/shared/lib/utils";

/** Sizes the `sirtone-cover` protocol serves (docs/03 §5.6). */
const SIZES = [64, 128, 256, 512, 1024] as const;

/** Smallest served size that covers `px` CSS pixels on this display. */
export function coverUrl(hash: string, px: number): string {
  const need = px * (window.devicePixelRatio || 1);
  const size = SIZES.find((s) => s >= need) ?? 1024;
  return `http://sirtone-cover.localhost/${hash}/${size}`;
}

/**
 * Album artwork (DESIGN.md §9): the cached WebP from `sirtone-cover://`, or the
 * `artwork-placeholder` square with a note icon while there is none or it fails to load.
 */
export function Cover({
  hash,
  px,
  className,
  rounded = "sm",
}: {
  hash: string | null | undefined;
  px: number;
  className?: string;
  rounded?: "sm" | "md";
}) {
  const [failed, setFailed] = useState<string | null>(null);
  const radius = rounded === "md" ? "rounded-(--radius-md)" : "rounded-(--radius-sm)";
  const box = cn("shrink-0 overflow-hidden", radius, className);
  if (!hash || failed === hash) {
    return (
      <span
        className={cn(
          box,
          "flex items-center justify-center bg-artwork-placeholder text-text-tertiary",
        )}
        style={{ width: px, height: px }}
      >
        <Music2 size={Math.max(16, Math.round(px / 3))} strokeWidth={1.5} />
      </span>
    );
  }
  return (
    <img
      src={coverUrl(hash, px)}
      width={px}
      height={px}
      alt=""
      loading="lazy"
      decoding="async"
      onError={() => setFailed(hash)}
      className={cn(box, "bg-artwork-placeholder object-cover")}
    />
  );
}
