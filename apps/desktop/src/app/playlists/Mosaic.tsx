import { cn } from "@/shared/lib/utils";
import { Cover } from "@/shared/ui/Cover";

/**
 * Playlist cover (FR-PLS-001 «мозаика по умолчанию»; Spotify, `01b` F-13): four different album
 * covers of the first tracks as 2×2, otherwise the first cover or the placeholder.
 */
export function Mosaic({
  covers,
  px,
  className,
}: {
  covers: string[];
  px: number;
  className?: string | undefined;
}) {
  if (covers.length < 4) {
    return <Cover hash={covers[0]} px={px} rounded="md" className={className ?? ""} />;
  }
  return (
    <div
      className={cn("grid grid-cols-2 overflow-hidden rounded-(--radius-md)", className)}
      style={{ width: px, height: px }}
    >
      {covers.slice(0, 4).map((h) => (
        <Cover key={h} hash={h} px={px / 2} rounded="sm" className="rounded-none" />
      ))}
    </div>
  );
}
