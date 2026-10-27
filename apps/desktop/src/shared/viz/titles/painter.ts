import { rand } from "./rng";
import type { Prim, TextureKind } from "./texture";
import { tileOf } from "./texture";

/**
 * What the scenes draw with (D-122). The scenes only talk to this interface, so the cut list and
 * the scenes can be tested without a canvas (a coarse raster stands in, see `scenes.test.ts`), and
 * the real `CanvasPainter` stays a thin layer over the 2D context. Coordinates are CSS-free
 * pixels of the canvas.
 */
export type TextOpts = {
  size: number;
  color: string;
  weight: number;
  /** Horizontal scale of the one family: ≈ 1.25 wide, ≈ 0.6 narrow (D-122). */
  xScale: number;
  align: "left" | "right" | "center";
  alpha?: number;
  /** Hollow letters: stroke only. */
  stroke?: number;
};

export interface Painter {
  readonly w: number;
  readonly h: number;
  fillRect(x: number, y: number, w: number, h: number, color: string, alpha?: number): void;
  strokeRect(x: number, y: number, w: number, h: number, color: string, lineWidth: number): void;
  /** `pts` — x0, y0, x1, y1, … */
  poly(pts: number[], color: string, alpha?: number): void;
  polyGradient(pts: number[], from: string, to: string, angleDeg: number): void;
  line(pts: number[], color: string, lineWidth: number, alpha?: number): void;
  circle(cx: number, cy: number, r: number, color: string, alpha?: number): void;
  ring(cx: number, cy: number, r: number, color: string, lineWidth: number): void;
  /** Baseline at `y`. */
  text(s: string, x: number, y: number, o: TextOpts): void;
  measure(s: string, size: number, weight: number, xScale: number): number;
  clipPoly(pts: number[], draw: () => void): void;
  /** A texture over the box, ink `color`, one tile = `tile` px; drawn inside the current clip. */
  texture(
    kind: TextureKind,
    seed: number,
    phase: number,
    box: [x: number, y: number, w: number, h: number],
    tile: number,
    color: string,
  ): void;
  /** Glitch: `n` horizontal strips of what is already drawn, shifted sideways. */
  slices(n: number, maxShift: number, seed: number): void;
  grain(amount: number, seed: number): void;
  scanlines(alpha: number, step: number): void;
}

const FONT = 'Geologica, "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';

export class CanvasPainter implements Painter {
  private noise: HTMLCanvasElement | null = null;

  constructor(
    private ctx: CanvasRenderingContext2D,
    readonly w: number,
    readonly h: number,
  ) {}

  private set(color: string, alpha = 1): void {
    this.ctx.fillStyle = color;
    this.ctx.globalAlpha = alpha;
  }

  fillRect(x: number, y: number, w: number, h: number, color: string, alpha = 1): void {
    this.set(color, alpha);
    this.ctx.fillRect(x, y, w, h);
    this.ctx.globalAlpha = 1;
  }

  strokeRect(x: number, y: number, w: number, h: number, color: string, lineWidth: number): void {
    const c = this.ctx;
    c.strokeStyle = color;
    c.lineWidth = lineWidth;
    c.lineJoin = "miter";
    c.strokeRect(x, y, w, h);
  }

  private path(pts: number[]): void {
    const c = this.ctx;
    c.beginPath();
    c.moveTo(pts[0] ?? 0, pts[1] ?? 0);
    for (let i = 2; i < pts.length; i += 2) c.lineTo(pts[i] ?? 0, pts[i + 1] ?? 0);
    c.closePath();
  }

  poly(pts: number[], color: string, alpha = 1): void {
    this.set(color, alpha);
    this.path(pts);
    this.ctx.fill();
    this.ctx.globalAlpha = 1;
  }

  polyGradient(pts: number[], from: string, to: string, angleDeg: number): void {
    const c = this.ctx;
    let x0 = Number.POSITIVE_INFINITY;
    let x1 = Number.NEGATIVE_INFINITY;
    let y0 = Number.POSITIVE_INFINITY;
    let y1 = Number.NEGATIVE_INFINITY;
    for (let i = 0; i < pts.length; i += 2) {
      x0 = Math.min(x0, pts[i] ?? 0);
      x1 = Math.max(x1, pts[i] ?? 0);
      y0 = Math.min(y0, pts[i + 1] ?? 0);
      y1 = Math.max(y1, pts[i + 1] ?? 0);
    }
    const a = (angleDeg * Math.PI) / 180;
    const cx = (x0 + x1) / 2;
    const cy = (y0 + y1) / 2;
    const r = Math.hypot(x1 - x0, y1 - y0) / 2;
    const g = c.createLinearGradient(
      cx - Math.cos(a) * r,
      cy - Math.sin(a) * r,
      cx + Math.cos(a) * r,
      cy + Math.sin(a) * r,
    );
    g.addColorStop(0, from);
    g.addColorStop(1, to);
    c.globalAlpha = 1;
    c.fillStyle = g;
    this.path(pts);
    c.fill();
  }

  line(pts: number[], color: string, lineWidth: number, alpha = 1): void {
    const c = this.ctx;
    c.globalAlpha = alpha;
    c.strokeStyle = color;
    c.lineWidth = lineWidth;
    c.lineJoin = "round";
    c.lineCap = "round";
    c.beginPath();
    c.moveTo(pts[0] ?? 0, pts[1] ?? 0);
    for (let i = 2; i < pts.length; i += 2) c.lineTo(pts[i] ?? 0, pts[i + 1] ?? 0);
    c.stroke();
    c.globalAlpha = 1;
  }

  circle(cx: number, cy: number, r: number, color: string, alpha = 1): void {
    this.set(color, alpha);
    this.ctx.beginPath();
    this.ctx.arc(cx, cy, Math.max(0, r), 0, Math.PI * 2);
    this.ctx.fill();
    this.ctx.globalAlpha = 1;
  }

  ring(cx: number, cy: number, r: number, color: string, lineWidth: number): void {
    const c = this.ctx;
    c.strokeStyle = color;
    c.lineWidth = lineWidth;
    c.beginPath();
    c.arc(cx, cy, Math.max(0, r), 0, Math.PI * 2);
    c.stroke();
  }

  text(s: string, x: number, y: number, o: TextOpts): void {
    const c = this.ctx;
    c.save();
    c.translate(x, y);
    c.scale(o.xScale, 1);
    c.font = `${o.weight} ${o.size}px ${FONT}`;
    c.textAlign = o.align;
    c.textBaseline = "alphabetic";
    c.globalAlpha = o.alpha ?? 1;
    if (o.stroke) {
      c.lineJoin = "round";
      c.strokeStyle = o.color;
      c.lineWidth = o.stroke;
      c.strokeText(s, 0, 0);
    } else {
      c.fillStyle = o.color;
      c.fillText(s, 0, 0);
    }
    c.restore();
  }

  measure(s: string, size: number, weight: number, xScale: number): number {
    const c = this.ctx;
    c.save();
    c.font = `${weight} ${size}px ${FONT}`;
    const w = c.measureText(s).width * xScale;
    c.restore();
    return w;
  }

  clipPoly(pts: number[], draw: () => void): void {
    const c = this.ctx;
    c.save();
    this.path(pts);
    c.clip();
    draw();
    c.restore();
  }

  texture(
    kind: TextureKind,
    seed: number,
    phase: number,
    box: [number, number, number, number],
    tile: number,
    color: string,
  ): void {
    const [bx, by, bw, bh] = box;
    const prims: Prim[] = tileOf(kind, seed, phase);
    const c = this.ctx;
    c.globalAlpha = 1;
    c.fillStyle = color;
    c.beginPath();
    const nx = Math.ceil(bw / tile);
    const ny = Math.ceil(bh / tile);
    for (let iy = 0; iy < ny; iy++) {
      for (let ix = 0; ix < nx; ix++) {
        const ox = bx + ix * tile;
        const oy = by + iy * tile;
        for (const p of prims) {
          if ("r" in p)
            c.rect(ox + p.r[0] * tile, oy + p.r[1] * tile, p.r[2] * tile, p.r[3] * tile);
          else {
            c.moveTo(ox + (p.c[0] + p.c[2]) * tile, oy + p.c[1] * tile);
            c.arc(ox + p.c[0] * tile, oy + p.c[1] * tile, p.c[2] * tile, 0, Math.PI * 2);
          }
        }
      }
    }
    c.fill();
  }

  slices(n: number, maxShift: number, seed: number): void {
    const c = this.ctx;
    const cv = c.canvas;
    for (let i = 0; i < n; i++) {
      const y = Math.floor(rand(seed, i, 1) * this.h);
      const sh = Math.max(4, Math.floor(rand(seed, i, 2) * this.h * 0.12));
      const dx = Math.round((rand(seed, i, 3) - 0.5) * 2 * maxShift);
      c.drawImage(cv, 0, y, this.w, sh, dx, y, this.w, sh);
    }
  }

  grain(amount: number, seed: number): void {
    if (!this.noise) {
      const n = document.createElement("canvas");
      n.width = n.height = 128;
      const nc = n.getContext("2d");
      if (!nc) return;
      const img = nc.createImageData(128, 128);
      for (let i = 0; i < img.data.length; i += 4) {
        const v = rand(i, 0x9a) > 0.5 ? 255 : 0;
        img.data[i] = img.data[i + 1] = img.data[i + 2] = v;
        img.data[i + 3] = 255;
      }
      nc.putImageData(img, 0, 0);
      this.noise = n;
    }
    const c = this.ctx;
    const pat = c.createPattern(this.noise, "repeat");
    if (!pat) return;
    c.save();
    c.translate(Math.floor(rand(seed, 1) * 128), Math.floor(rand(seed, 2) * 128));
    c.globalAlpha = amount;
    c.fillStyle = pat;
    c.fillRect(-128, -128, this.w + 256, this.h + 256);
    c.restore();
    c.globalAlpha = 1;
  }

  scanlines(alpha: number, step: number): void {
    const c = this.ctx;
    c.globalAlpha = alpha;
    c.fillStyle = "#000";
    for (let y = 0; y < this.h; y += step * 2) c.fillRect(0, y, this.w, step);
    c.globalAlpha = 1;
  }
}
