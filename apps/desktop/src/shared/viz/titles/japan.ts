import { TITLES } from "./config";
import type { Painter } from "./painter";
import { hash32, pick } from "./rng";
import type { SceneState } from "./scenes";

/**
 * The Japanese layer of «Титры» (D-123, `06` §7.8): words, marks and shapes in the manner of an
 * anime opening — a vertical word at the edge, the episode counter «第N幕», a katakana tag line, a
 * seal (hanko), the sun disc or the wave pattern (seigaiha). Only content: the colours, the timing
 * and the effects are the style's own. Everything is a pure function of (track seed, card), drawn
 * still for the whole card — no new motion, nothing that could flash. The marks are decoration
 * for the eye, not text to read (the Russian caption always stays).
 */
const J = () => TITLES.japan;

/** 1…99 in kanji numerals: 九 · 十 · 十二 · 二十 · 二十三. */
export function kanjiNumber(n: number): string {
  const d = J().numerals;
  const v = ((Math.max(1, Math.floor(n)) - 1) % 99) + 1;
  if (v <= 10) return d[v] as string;
  const tens = Math.floor(v / 10);
  const one = v % 10;
  return (
    (tens > 1 ? (d[tens] as string) : "") + (d[10] as string) + (one > 0 ? (d[one] as string) : "")
  );
}

export type JapanCard = {
  /** The vertical word, one glyph per row. */
  column: string[];
  tag: string;
  episode: string;
  seal: string;
  /** 0 seal, 1 sun disc, 2 waves. */
  shape: 0 | 1 | 2;
};

export function japanOf(seed: number, k: number): JapanCard {
  const j = J();
  const word = pick(j.words, seed, k, 0x4a1);
  const t1 = pick(j.tags, seed, k, 0x4a2);
  const t2 = pick(j.tags, seed, k, 0x4a3);
  return {
    column: Array.from(word),
    tag: t1 === t2 ? t1 : `${t1}・${t2}`,
    episode: `${j.episode[0]}${kanjiNumber(k + 1)}${j.episode[1]}`,
    seal: pick(j.seals, seed, k, 0x4a4),
    shape: (hash32(seed, k, 0x4a5) % 3) as 0 | 1 | 2,
  };
}

/** The small Japanese tag in front of a role caption: 曲・Трек. */
export const roleTag = (role: keyof typeof TITLES.japan.roles): string => J().roles[role];

export type JapanColors = { ink: string; accent: string; onAccent: string };

const GLYPH = 0.78;

/** The shape, drawn under the titles at the upper corner opposite to them. */
export function drawJapanShape(p: Painter, s: SceneState, c: JapanColors): void {
  if (s.quiet) return;
  const { w, h } = p;
  const j = japanOf(s.seed, s.card.k);
  const size = J().seal_size * h;
  const cx = (s.card.flip ? 0.2 : 0.8) * w;
  const top = 0.1 * h;
  if (j.shape === 0) {
    p.fillRect(cx - size / 2, top, size, size, c.accent);
    p.strokeRect(
      cx - size / 2 + 0.008 * h,
      top + 0.008 * h,
      size - 0.016 * h,
      size - 0.016 * h,
      c.onAccent,
      0.004 * h,
    );
    p.text(j.seal, cx, top + size * GLYPH, {
      size: size * 0.66,
      color: c.onAccent,
      weight: 700,
      xScale: 1,
      align: "center",
    });
  } else if (j.shape === 1) {
    p.circle(cx, top + size / 2, size / 2, c.accent);
  } else {
    const d = size * 0.5;
    const bw = size * 1.9;
    p.clipPoly(
      [cx - bw / 2, top, cx + bw / 2, top, cx + bw / 2, top + size, cx - bw / 2, top + size],
      () => {
        for (let r = 0; r < 4; r++) {
          for (let q = -1; q < 5; q++) {
            const x = cx - bw / 2 + q * d + (r % 2) * (d / 2);
            const y = top + r * (d / 2) + d / 2;
            for (const m of [1, 0.68, 0.36]) p.ring(x, y, (d / 2) * m, c.accent, 0.003 * h);
          }
        }
      },
    );
  }
}

/** The words: the episode counter, the vertical word and the katakana tag line. */
export function drawJapanText(p: Painter, s: SceneState, color: string, weight: number): void {
  if (s.quiet) return;
  const { w, h } = p;
  const j = japanOf(s.seed, s.card.k);
  const side = s.card.flip ? 0.045 * w : 0.955 * w;
  const tagSize = J().tag_size * h;
  p.text(j.episode, side, 0.075 * h, {
    size: tagSize,
    color,
    weight,
    xScale: 1,
    align: s.card.flip ? "left" : "right",
  });
  const col = J().column_size * h;
  j.column.forEach((g, i) => {
    p.text(g, side + (s.card.flip ? 0.5 : -0.5) * col, 0.2 * h + i * col * 1.18, {
      size: col,
      color,
      weight,
      xScale: 1,
      align: "center",
    });
  });
  p.text(`「${j.tag}」`, (s.card.flip ? 0.95 : 0.05) * w, 0.965 * h, {
    size: tagSize,
    color,
    weight,
    xScale: 1,
    align: s.card.flip ? "right" : "left",
  });
}
