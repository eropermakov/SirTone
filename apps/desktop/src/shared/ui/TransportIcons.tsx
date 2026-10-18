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
