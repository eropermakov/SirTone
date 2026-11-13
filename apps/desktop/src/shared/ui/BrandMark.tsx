import { cn } from "@/shared/lib/utils";

/**
 * The Сиртон mark (owner logo 1.0, D-175): the open ring «С», the inner arc and the Sir.kov
 * dot. The ring takes the text colour (black on light, white on dark); the dot is always the
 * Sir.kov burgundy, never the user's accent. From 48 px the main drawing, below — the small
 * one (thicker stroke, bigger dot), as the logo guide says.
 */
export function BrandMark({ px, className }: { px: number; className?: string }) {
  const small = px < 48;
  return (
    <svg
      aria-hidden
      viewBox="0 0 256 256"
      width={px}
      height={px}
      className={cn("shrink-0", className)}
    >
      {small ? (
        <g transform="translate(4 0)" fill="currentColor">
          <path d="M220.85 65.37A112 112 0 1 0 220.85 190.63A14 14 0 0 0 197.64 174.97A84 84 0 1 1 197.64 81.03A14 14 0 0 0 220.85 65.37Z" />
          <path d="M69.75 85.68A72 72 0 0 0 69.75 170.32A12 12 0 0 0 89.17 156.21A48 48 0 0 1 89.17 99.79A12 12 0 0 0 69.75 85.68Z" />
          <circle cx="128" cy="128" r="23" fill="var(--accent-sirkov-fill)" />
        </g>
      ) : (
        <g transform="translate(5 0)" fill="currentColor">
          <path d="M223.26 73A110 110 0 1 0 223.26 183A10 10 0 0 0 205.94 173A90 90 0 1 1 205.94 83A10 10 0 0 0 223.26 73Z" />
          <path d="M67.62 88.79A72 72 0 0 0 67.62 167.21A10 10 0 0 0 84.39 156.32A52 52 0 0 1 84.39 99.68A10 10 0 0 0 67.62 88.79Z" />
          <circle cx="128" cy="128" r="22" fill="var(--accent-sirkov-fill)" />
        </g>
      )}
    </svg>
  );
}
