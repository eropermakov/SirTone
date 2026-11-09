import { type CSSProperties, useEffect, useRef } from "react";

/**
 * «Большая надпись раздела» (DESIGN.md §13, FR-LOOK-006): the section name at the top left of the
 * content, 14,5 % of its width, one line, fading in from the top edge; a long name shrinks to fit.
 * The container (`<main>`) is a size container, so `cqw` is the content width. Not on
 * `tpl.immersive` screens («Моя волна»).
 *
 * The lists below scroll inside their own box; the title leaves with that scroll (D-161): a list
 * calls `announceScroll(scrollTop)` and the title pulls itself up by as much.
 */
/** Average glyph width of Geologica bold, in em: how many characters fit in the width. */
const GLYPH_EM = 0.62;
const SCROLL_EVENT = "sirtone:list-scroll";

/** A list tells the title how far it has scrolled. */
export function announceScroll(top: number): void {
  window.dispatchEvent(new CustomEvent(SCROLL_EVENT, { detail: top }));
}

export function SectionTitle({ children }: { children: string }) {
  const ref = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    const on = (e: Event) => {
      const el = ref.current;
      if (!el) return;
      const top = (e as CustomEvent<number>).detail;
      el.style.marginTop = `-${Math.min(Math.max(top, 0), el.offsetHeight + 8)}px`;
    };
    window.addEventListener(SCROLL_EVENT, on);
    return () => window.removeEventListener(SCROLL_EVENT, on);
  }, []);
  const fit = 100 / (Math.max(children.length, 1) * GLYPH_EM);
  return (
    <h1
      ref={ref}
      className="section-title"
      style={{ "--section-title-fit": `${fit.toFixed(2)}cqw` } as CSSProperties}
    >
      {children}
    </h1>
  );
}
