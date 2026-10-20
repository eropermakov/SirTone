import type React from "react";
import type { SVGProps } from "react";

/**
 * Solid transport glyphs with rounded corners (owner's reference, D-75): double triangles for
 * previous/next, two bars for pause, a soft triangle for play. They fill with `currentColor` and
 * round every corner by stroking the shape with the same colour.
 */
type Props = SVGProps<SVGSVGElement> & { size?: number };

function Glyph({ size = 24, children, ...rest }: Props) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="currentColor"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinejoin="round"
      strokeLinecap="round"
      aria-hidden
      {...rest}
    >
      {children}
    </svg>
  );
}

export const RewindIcon = (p: Props) => (
  <Glyph {...p}>
    <path d="M3 12 11 6v12Z" />
    <path d="M12 12l8-6v12Z" />
  </Glyph>
);

export const ForwardIcon = (p: Props) => (
  <Glyph {...p}>
    <path d="M21 12 13 6v12Z" />
    <path d="M12 12 4 6v12Z" />
  </Glyph>
);

export const PauseIcon = (p: Props) => (
  <Glyph {...p} strokeWidth={0}>
    <rect x="5" y="3.5" width="5" height="17" rx="1.6" />
    <rect x="14" y="3.5" width="5" height="17" rx="1.6" />
  </Glyph>
);

export const PlayIcon = (p: Props) => (
  <Glyph {...p}>
    <path d="M7 5 19 12 7 19Z" />
  </Glyph>
);

/**
 * Play ⇄ pause as one shape (owner, 2026-10-20, D-106; copies: YouTube's player button and
 * Android's AnimatedVectorDrawable play↔pause — the vector points travel, nothing swaps). The
 * play triangle is cut into two four-point halves; each half's points move to one pause bar.
 * The `d` property animates in CSS (Chromium), so a press answers at once and the shape flows
 * over `--dur-base`; reduced motion makes it a plain swap.
 */
const MORPH = {
  play: ["M7 5 13 8.5 13 15.5 7 19Z", "M13 8.5 19 12 19 12 13 15.5Z"],
  pause: ["M5.8 4.3 9.2 4.3 9.2 19.7 5.8 19.7Z", "M14.8 4.3 18.2 4.3 18.2 19.7 14.8 19.7Z"],
} as const;

export function PlayPauseIcon({ playing, ...p }: Props & { playing: boolean }) {
  const shape = playing ? MORPH.pause : MORPH.play;
  return (
    <Glyph {...p} strokeWidth={1.6} data-playing={playing}>
      {shape.map((d, i) => (
        <path
          // biome-ignore lint/suspicious/noArrayIndexKey: two fixed halves of one shape
          key={i}
          style={
            {
              d: `path("${d}")`,
              transition: "d var(--dur-base) var(--ease-out)",
            } as React.CSSProperties
          }
        />
      ))}
    </Glyph>
  );
}
