import type { CSSProperties } from "react";

/**
 * «Большая надпись раздела» (DESIGN.md §13, FR-LOOK-006): the section name at the top left of the
 * content, 14,5 % of its width, one line, fading in from the top edge; a long name shrinks to fit.
 * The container (`<main>`) is a size container, so `cqw` is the content width. Not on
 * `tpl.immersive` screens («Моя волна»).
 */
/** Average glyph width of Geologica bold, in em: how many characters fit in the width. */
const GLYPH_EM = 0.62;

export function SectionTitle({ children }: { children: string }) {
  const fit = 100 / (Math.max(children.length, 1) * GLYPH_EM);
  return (
    <h1
      className="section-title"
      style={{ "--section-title-fit": `${fit.toFixed(2)}cqw` } as CSSProperties}
    >
      {children}
    </h1>
  );
}
