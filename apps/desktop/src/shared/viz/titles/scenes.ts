import { type StyleId, TITLES, type TitlesQuality } from "./config";
import { drawJapanShape, drawJapanText } from "./japan";
import type { Painter } from "./painter";
import type { Act, Card } from "./plan";
import { hash32, rand } from "./rng";
import { TEXTURE_KINDS } from "./texture";

/**
 * The two styles of the «Титры» mode (D-122, `01a` §B.5), drawn layer by layer: field → cut-outs
 * or frame → textures → accent shapes → pulse line → titles → effects. A scene is a function of
 * the state only (no clock inside): the same state draws the same picture.
 */
export type SceneState = {
  style: StyleId;
  seed: number;
  card: Card;
  /** Beats since the card began (float) and the whole beat index of the track. */
  cardBeat: number;
  beatIdx: number;
  /** Frame counter at the drawing rate and the frames since the card began. */
  frame: number;
  sinceCard: number;
  /** Title lines (upper case) and the small role caption above them. */
  lines: string[];
  roleLabel: string;
  /** Structure 0…1, fixed for a whole card. */
  density: number;
  /** The live loudness 0…1, for continuous amplitudes only. */
  energy: number;
  act: Act;
  quiet: boolean;
  /** Milliseconds since the last hit of each band. */
  kickAge: number;
  snareAge: number;
  bands: [number, number, number];
  wave: Uint8Array | null;
  q: TitlesQuality;
  reduced: boolean;
  /** The colour of the frame steps every this many beats (≤ 2.5 Hz, WCAG 2.3.1). */
  stepEvery: number;
  /** The climax's pink fill 0…1 (poster): granted through the flash budget by the engine. */
  fill: number;
};

const LINE_HEIGHT = 0.92;

function widest(p: Painter, lines: string[], weight: number, xScale: number): number {
  let m = 1;
  for (const l of lines) m = Math.max(m, p.measure(l, 100, weight, xScale) / 100);
  return m;
}

/** The largest size ≤ `maxSize` that fits the lines into `maxW` × `maxH`. */
function fitSize(
  p: Painter,
  lines: string[],
  weight: number,
  xScale: number,
  maxW: number,
  maxH: number,
  maxSize: number,
): number {
  const bySize = maxW / widest(p, lines, weight, xScale);
  const byHeight = maxH / (Math.max(1, lines.length) * LINE_HEIGHT);
  return Math.max(8, Math.min(maxSize, bySize, byHeight));
}

/** Entry of the title: slides in over `enterFrames` frames (a step each, not a glide). */
function enterOffset(s: SceneState, w: number): number {
  if (s.reduced) return 0;
  const n = s.card.enterFrames;
  const left = Math.max(0, n - s.sinceCard) / n;
  return left * w * 0.1 * (s.card.flip ? 1 : -1);
}

const pulse = (age: number, ms: number) => Math.max(0, 1 - age / ms);

// ───────────────────────────── «Афиша» (style 2) ─────────────────────────────

export function renderPoster(p: Painter, s: SceneState): void {
  const c = TITLES.styles.poster;
  const { w, h } = p;
  p.fillRect(0, 0, w, h, c.field);

  const bar = Math.floor(s.cardBeat / TITLES.card.beats_per_bar);
  const jump = s.reduced ? 0 : (rand(s.seed, s.card.k, bar, 0x3a) - 0.5) * 0.08 * w;
  const side = s.card.flip ? -1 : 1;
  const fw = 0.56 * w;
  const fh = 0.62 * h;
  const fx = w / 2 - fw / 2 + side * 0.07 * w + jump;
  const fy = 0.17 * h;
  const step = Math.floor(s.cardBeat / s.stepEvery);
  const stepColor = c.frame_steps[(step + s.card.hue) % c.frame_steps.length] as string;
  const [g1, g2, ink] = c.grays as [string, string, string];

  if (!s.quiet) {
    // The frame is a thin border; its colour steps on the beat (never a flooded block).
    p.strokeRect(fx, fy, fw, fh, stepColor, c.frame_width * h);
    if (s.card.template === 0) {
      p.fillRect(fx + 0.58 * fw, fy + 0.1 * fh, 0.16 * fw, 0.8 * fh, ink);
      p.poly(
        [
          fx + 0.08 * fw,
          fy + 0.9 * fh,
          fx + 0.5 * fw,
          fy + 0.9 * fh,
          fx + 0.5 * fw,
          fy + 0.35 * fh,
        ],
        g1,
      );
      if (s.density > 0.3)
        p.fillRect(fx + 0.64 * fw, fy + 0.18 * fh, 0.012 * w, 0.26 * fh, c.white);
    } else if (s.card.template === 1) {
      const cx = fx + 0.5 * fw;
      const cy = fy + 0.5 * fh;
      for (const [i, m] of [1, 0.7, 0.4].entries()) {
        p.ring(cx, cy, 0.34 * fh * m, i % 2 ? g2 : g1, 0.014 * h);
      }
      p.circle(cx, cy, 0.05 * fh, ink);
      if (s.density > 0.3) p.fillRect(fx + 0.1 * fw, fy + 0.1 * fh, 0.12 * fw, 0.18 * fh, ink);
    } else {
      for (let i = 0; i < 5; i++) {
        const bh = (0.3 + 0.5 * rand(s.seed, s.card.k, i, 0xb4)) * fh;
        p.fillRect(fx + (0.08 + i * 0.17) * fw, fy + fh - bh, 0.11 * fw, bh, i % 2 ? g2 : g1);
      }
      p.fillRect(fx + 0.06 * fw, fy + 0.08 * fh, 0.2 * fw, 0.22 * fh, ink);
    }
  } else {
    p.fillRect(fx + 0.4 * fw, fy + 0.5 * fh, 0.05 * fw, 0.05 * fw, g1);
  }

  // The climax: the pink torn shape grows from the bottom-right corner (slowly, a budgeted event).
  if (s.fill > 0) {
    const g = s.fill;
    const pts = [w, h, w * (1 - 0.55 * g), h];
    for (let i = 1; i <= 6; i++) {
      const t = i / 6;
      pts.push(
        w * (1 - 0.55 * g * (1 - t) - 0.1 * rand(s.seed, i, 0x71) * g),
        h * (1 - 0.6 * g * t),
      );
    }
    p.poly(pts, c.pulse, 0.92);
  }

  // The pulse line: the sound's own waveform, spiking on the kick.
  const baseY = 0.9 * h;
  const amp =
    h *
    (0.012 + (s.reduced ? 0 : 0.06 * s.bands[0] + 0.1 * pulse(s.kickAge, TITLES.onset.kick_ms)));
  const n = 96;
  const pts: number[] = [];
  for (let i = 0; i < n; i++) {
    let v = 0;
    if (s.wave && !s.reduced)
      v = ((s.wave[Math.floor((i * s.wave.length) / n)] ?? 128) - 128) / 127;
    else if (!s.reduced) v = Math.sin(i * 0.5 + s.frame * 0.3) * 0.4 * s.energy;
    pts.push((i / (n - 1)) * w, baseY + v * amp);
  }
  p.line(pts, c.pulse, 0.005 * h);

  drawJapanShape(p, s, { ink, accent: c.pulse, onAccent: ink });
  drawJapanText(p, s, c.text, 700);

  if (!s.quiet && s.lines.length > 0) {
    const wt = 800;
    const xs = TITLES.text.xscale_narrow;
    const outline = s.card.variant === "outline";
    const size = fitSize(p, s.lines, wt, xs, 0.88 * w, 0.62 * h, outline ? 0.4 * h : 0.34 * h);
    const blockH = s.lines.length * size * LINE_HEIGHT;
    const top = 0.5 * h - blockH / 2;
    const x = (s.card.flip ? 0.95 * w : 0.05 * w) + enterOffset(s, w);
    const align = s.card.flip ? "right" : "left";
    const roleSize = TITLES.text.role_size * h;
    p.text(s.roleLabel, x, top - roleSize * 0.6, {
      size: roleSize,
      color: c.text,
      weight: 600,
      xScale: 1,
      align,
    });
    s.lines.forEach((line, i) => {
      const y = top + (i + 1) * size * LINE_HEIGHT;
      const o = { size, color: c.text, weight: wt, xScale: xs, align } as const;
      if (!s.reduced && s.card.variant === "echo" && pulse(s.kickAge, 300) > 0) {
        p.text(line, x - 0.014 * w, y, { ...o, alpha: 0.3 });
        p.text(line, x - 0.028 * w, y, { ...o, alpha: 0.15 });
      }
      if (outline) p.text(line, x, y, { ...o, stroke: 0.004 * h });
      else p.text(line, x, y, o);
    });
  }
  if (s.q.grain) p.grain(c.grain, s.reduced ? 1 : s.frame);
}

// ───────────────────────────── «Удар» (style 1) ─────────────────────────────

export function renderShock(p: Painter, s: SceneState): void {
  const c = TITLES.styles.shock;
  const { w, h } = p;
  p.fillRect(0, 0, w, h, c.field);

  const hy = 0.85 * h;
  p.fillRect(0, hy, w, 0.006 * h, c.ink);
  // The small mark against the huge field: it jumps along the horizon, one jump per beat.
  const mx = w * (0.1 + 0.8 * rand(s.seed, s.card.k, s.beatIdx, 0x31));
  p.fillRect(mx, hy - 0.07 * h, 0.012 * h, 0.07 * h, c.marker);
  if (!s.quiet && !s.reduced && s.kickAge < TITLES.onset.kick_ms) {
    const px = w * (0.1 + 0.8 * rand(s.seed, s.card.k, s.beatIdx - 1, 0x31));
    p.fillRect(Math.min(px, mx), hy - 0.012 * h, Math.abs(mx - px), 0.006 * h, c.marker, 0.7);
  }

  const cutouts: number[][] = [];
  if (!s.quiet && s.density > 0.2) {
    // Cut-outs: diagonal strips through the yellow field; the picture lives only inside them.
    const strips = s.density < 0.45 ? 1 : s.density < 0.75 ? 2 : 3;
    const sw = (c.slits.max_area * w * 0.9) / strips;
    const lean = h / Math.tan((c.slits.angle_deg * Math.PI) / 180);
    const flick = s.q.flicker && !s.reduced;
    // The texture flips every 2–3 frames (flicker) or once a beat (still): same ink coverage.
    const stepNo = flick ? Math.floor(s.frame / (2 + (hash32(s.seed, s.card.k) % 2))) : s.beatIdx;
    for (let i = 0; i < strips; i++) {
      const x0 = w * (0.12 + (0.7 * (i + rand(s.seed, s.card.k, i, 0x52))) / strips);
      const pts = [x0, 0, x0 + sw, 0, x0 + sw - lean, h, x0 - lean, h];
      cutouts.push(pts);
      let kind =
        TEXTURE_KINDS[hash32(s.seed, s.card.k, i, stepNo) % TEXTURE_KINDS.length] ?? "bars";
      if (kind === "dots" && !s.q.halftone) kind = "bars";
      p.clipPoly(pts, () => {
        p.fillRect(0, 0, w, h, c.mass);
        p.texture(
          kind,
          s.seed + i,
          flick ? s.frame : 0,
          [x0 - lean, 0, sw + lean, h],
          0.04 * h,
          c.texture,
        );
      });
    }
  }

  // An accent shape with a two-colour gradient and a channel-shifted rim, once the frame is busy.
  if (!s.quiet && s.density >= 0.45) {
    const sz = h * (0.22 + 0.16 * s.density);
    const pts = [w * 0.74, hy, w * 0.98, hy, w * 0.98, hy - sz];
    const gr = c.gradients[(s.beatIdx + s.card.k) % c.gradients.length] as [string, string];
    const d = 0.004 * h;
    p.poly(
      pts.map((v, i) => (i % 2 === 0 ? v - d : v)),
      c.texture,
      0.9,
    );
    p.poly(
      pts.map((v, i) => (i % 2 === 0 ? v + d : v)),
      c.accents[0] as string,
      0.9,
    );
    p.polyGradient(pts, gr[0], gr[1], 300);
    const k = pulse(s.kickAge, TITLES.onset.kick_ms);
    if (!s.reduced && k > 0) {
      for (let r = 1; r <= 3; r++)
        p.ring(w * 0.9, hy - sz * 0.35, h * 0.05 * r * (1.4 - k * 0.5), c.ink, 0.005 * h);
    }
  }

  drawJapanShape(p, s, { ink: c.ink, accent: c.marker, onAccent: c.field });

  if (!s.quiet && s.lines.length > 0) {
    // The titles are black on yellow; where they cross a cut-out they turn yellow (still legible).
    const paintTitles = (ink: string) => {
      drawJapanText(p, s, ink, 700);
      const wt = 900;
      const xs = TITLES.text.xscale_wide;
      const outline = s.card.variant === "outline";
      const size = fitSize(
        p,
        s.lines,
        wt,
        xs,
        (outline ? 1.35 : 0.86) * w,
        0.6 * h,
        outline ? 0.46 * h : 0.2 * h,
      );
      const blockH = s.lines.length * size * LINE_HEIGHT;
      const top = 0.46 * h - blockH / 2;
      const x = (s.card.flip ? 0.94 * w : 0.06 * w) + enterOffset(s, w);
      const align = s.card.flip ? "right" : "left";
      const roleSize = TITLES.text.role_size * h;
      p.text(s.roleLabel, x, top - roleSize * 0.5, {
        size: roleSize,
        color: ink,
        weight: 700,
        xScale: 1,
        align,
      });
      s.lines.forEach((line, i) => {
        const y = top + (i + 1) * size * LINE_HEIGHT;
        const o = { size, color: ink, weight: wt, xScale: xs, align } as const;
        if (
          !s.reduced &&
          s.card.variant === "echo" &&
          (s.sinceCard < s.card.enterFrames + 6 || pulse(s.kickAge, 200) > 0)
        ) {
          for (const [j, a] of [0.5, 0.35, 0.25, 0.15].entries()) {
            p.text(line, x, y - (j + 1) * 0.05 * size, { ...o, alpha: a });
          }
        }
        if (!s.reduced && s.card.variant === "split" && s.snareAge < TITLES.onset.snare_ms) {
          p.text(line, x + 0.03 * w, y, { ...o, color: c.marker, alpha: 0.7 });
        }
        if (outline) {
          p.text(line, x, y, { ...o, stroke: 0.004 * h });
          if (i === 0) {
            // The small solid line over the hollow giant keeps the words readable.
            p.text(s.lines.join(" "), x, top - roleSize * 1.9, {
              size: 0.05 * h,
              color: ink,
              weight: 700,
              xScale: 1,
              align,
            });
          }
        } else p.text(line, x, y, o);
      });
    };
    paintTitles(c.text_on_field);
    for (const pts of cutouts) p.clipPoly(pts, () => paintTitles(c.field));
  }

  if (
    s.q.glitch &&
    !s.reduced &&
    (s.snareAge < TITLES.onset.snare_ms || (s.act === "climax" && s.kickAge < 90))
  ) {
    p.slices(3 + Math.floor(s.density * 3), 0.04 * w, s.frame);
  }
  if (s.q.glitch && !s.reduced && s.card.template === 2 && s.density > 0.3) p.scanlines(0.12, 2);
}

export function renderScene(p: Painter, s: SceneState): void {
  if (s.style === "shock") renderShock(p, s);
  else renderPoster(p, s);
}
