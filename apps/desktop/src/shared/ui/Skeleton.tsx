import { cn } from "@/shared/lib/utils";

/** Loading placeholder (DESIGN.md §9): `artwork-placeholder` with a soft pulse, never a spinner. */
export function Skeleton({ className }: { className?: string }) {
  return <div aria-hidden className={cn("skeleton rounded-(--radius-md)", className)} />;
}

/** Skeleton rows for a TrackList while its page loads. */
export function SkeletonRows({ count = 8 }: { count?: number }) {
  return (
    <div className="flex flex-col">
      {Array.from({ length: count }, (_, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: static placeholders
        <div key={i} className="flex h-(--row-height) items-center gap-3 px-3">
          <Skeleton className="h-3 w-8" />
          <div className="flex flex-1 flex-col gap-2">
            <Skeleton className="h-3 w-1/3" />
            <Skeleton className="h-3 w-1/5" />
          </div>
          <Skeleton className="h-3 w-12" />
        </div>
      ))}
    </div>
  );
}
