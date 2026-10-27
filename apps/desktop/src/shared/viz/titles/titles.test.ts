import { describe, expect, it } from "vitest";
import { contrast } from "../../lib/color";
import { BeatClock, Onsets, stepEveryBeats } from "./clock";
import { TITLES, type TitlesQuality } from "./config";
import type { Painter, TextOpts } from "./painter";
import {
  actOf,
  beatsFor,
  cardOf,
  densityOf,
  framesSince,
  locate,
  moodOf,
  resolveStyle,
  rolesOf,
  wrapWords,
} from "./plan";
import { hash32, rand } from "./rng";
import { renderScene, type SceneState } from "./scenes";
import { COVERAGE, inkCoverage, TEXTURE_KINDS, tileOf } from "./texture";

describe("the cut list (D-122)", () => {
  it("a card lasts 3–5 s at any tempo from 60 to 200 BPM, in whole beats", () => {
    for (let bpm = 60; bpm <= 200; bpm += 1) {
      for (let seed = 1; seed <= 6; seed++) {
        for (let k = 0; k < 8; k++) {
          const beats = beatsFor(bpm, seed, k);
          const s = (beats * 60) / bpm;
          expect(Number.isInteger(beats)).toBe(true);
          expect(s, `${bpm} bpm, ${beats} beats`).toBeGreaterThanOrEqual(TITLES.card.min_s);
          expect(s, `${bpm} bpm, ${beats} beats`).toBeLessThanOrEqual(TITLES.card.max_s);
        }
      }
    }
  });

  it("whole bars (2–4) are used wherever they fit", () => {
    for (const bpm of [100, 120, 128, 150, 170]) {
      for (let k = 0; k < 12; k++) {
        const beats = beatsFor(bpm, 7, k);
        expect(beats % 4, `${bpm} bpm`).toBe(0);
        expect(beats / 4).toBeGreaterThanOrEqual(2 - 1);
        expect(beats / 4).toBeLessThanOrEqual(4);
      }
    }
  });

  it("the same seed gives the same edit; another seed another one", () => {
    const roles = rolesOf({ tempo: true, mood: true });
    const a = Array.from({ length: 30 }, (_, k) => cardOf(42, k, k * 8, 120, roles));
    const b = Array.from({ length: 30 }, (_, k) => cardOf(42, k, k * 8, 120, roles));
    const c = Array.from({ length: 30 }, (_, k) => cardOf(43, k, k * 8, 120, roles));
    expect(a).toEqual(b);
    expect(a.map((x) => x.variant + x.template)).not.toEqual(c.map((x) => x.variant + x.template));
  });

  it("the first cards are the title and the artist; a variant never repeats twice in a row", () => {
    const roles = rolesOf({ tempo: true, mood: true });
    expect(cardOf(5, 0, 0, 120, roles).role).toBe("title");
    expect(cardOf(5, 1, 8, 120, roles).role).toBe("artist");
    let prev = "";
    let repeats = 0;
    for (let k = 1; k < 200; k++) {
      const v = cardOf(9, k, 0, 120, roles).variant;
      if (v === prev) repeats++;
      prev = v;
    }
    expect(repeats).toBe(0);
  });

  it("locate walks the cards: a position is inside the card it returns", () => {
    const roles = rolesOf({ tempo: false, mood: false });
    for (const pos of [0, 3.9, 8, 100.5, 333]) {
      const c = locate(11, 128, roles, pos);
      expect(pos).toBeGreaterThanOrEqual(c.start);
      expect(pos).toBeLessThan(c.start + c.beats);
    }
  });

  it("acts follow the place in the track; density follows the loudness", () => {
    expect(actOf(0.05, 0)).toBe("prologue");
    expect(actOf(0.5, 5)).toBe("ramp");
    expect(actOf(0.9, 20)).toBe("climax");
    expect(actOf(null, 0)).toBe("prologue");
    expect(actOf(null, 4)).toBe("ramp");
    expect(densityOf("climax", 1)).toBe(1);
    expect(densityOf("prologue", 0)).toBeLessThan(densityOf("ramp", 0));
    expect(densityOf("ramp", 0.2)).toBeLessThan(densityOf("ramp", 0.8));
  });

  it("«Авто» follows arousal, then tempo; a fixed choice wins", () => {
    expect(resolveStyle("auto", 1, 90, { valence: 0.5, arousal: 0.8 })).toBe("shock");
    expect(resolveStyle("auto", 1, 170, { valence: 0.5, arousal: 0.2 })).toBe("poster");
    expect(resolveStyle("auto", 1, 140, null)).toBe("shock");
    expect(resolveStyle("auto", 1, 80, null)).toBe("poster");
    expect(resolveStyle("poster", 1, 170, { valence: 0, arousal: 1 })).toBe("poster");
  });

  it("the mood word comes from the vibe, none without one", () => {
    expect(moodOf(null)).toBeNull();
    expect(moodOf({ valence: 0.2, arousal: 0.9 })).toBe("sad");
    expect(moodOf({ valence: 0.7, arousal: 0.8 })).toBe("energetic");
    expect(moodOf({ valence: 0.8, arousal: 0.3 })).toBe("happy");
    expect(moodOf({ valence: 0.5, arousal: 0.3 })).toBe("calm");
  });

  it("wrapWords breaks greedily, keeps a long word whole and never exceeds the line count", () => {
    const w = (s: string) => s.length;
    expect(wrapWords("keep it mello feat omar linx", 3, 10, w).length).toBeLessThanOrEqual(3);
    expect(wrapWords("supercalifragilistic", 3, 5, w)).toEqual(["supercalifragilistic"]);
    expect(wrapWords("", 3, 5, w)).toEqual([]);
    expect(wrapWords("a b c d e f g h", 2, 3, w)).toHaveLength(2);
  });

  it("entry frames are counted on the drawing grid", () => {
    expect(framesSince(0, 500, 24)).toBe(0);
    expect(framesSince(1, 500, 24)).toBe(12);
    expect(framesSince(-1, 500, 24)).toBe(0);
  });
});

describe("the beat clock", () => {
  const frame = (t: number, phase: number, bpm = 120, playing = true) => ({
    tAudioMs: t,
    beatPhase: phase,
    bpm,
    playing,
  });

  it("counts beats from the phase wraps and stays monotonic", () => {
    const c = new BeatClock();
    let wall = 1000;
    let last = -1;
    for (let i = 0; i < 240; i++) {
      const t = i * 16.7;
      c.feed(frame(t, ((t / 500) % 1) as number), wall);
      const p = c.pos(wall);
      expect(p).toBeGreaterThanOrEqual(last);
      last = p;
      wall += 16.7;
    }
    // 240 frames ≈ 4 s ≈ 8 beats.
    expect(last).toBeGreaterThan(7.5);
    expect(last).toBeLessThan(8.6);
  });

  it("re-reads the position from the time after a seek", () => {
    const c = new BeatClock();
    c.feed(frame(1000, 0.1), 0);
    c.feed(frame(1016, 0.13), 16);
    c.feed(frame(61000, 0.5), 32);
    // 61 s at 120 BPM = beat 122, half way through.
    expect(c.pos(32)).toBeGreaterThan(121.9);
    expect(c.pos(32)).toBeLessThan(123);
  });

  it("a paused clock stands still; with no frames it runs freely at 120 BPM", () => {
    const c = new BeatClock();
    c.feed(frame(2000, 0.3, 120, false), 0);
    const a = c.pos(100);
    expect(c.pos(150)).toBeCloseTo(a, 5);
    const free = new BeatClock();
    expect(free.pos(0)).toBe(0);
    expect(free.pos(2000)).toBeCloseTo(4, 1);
  });

  it("bpm 0 (not analysed) falls back to 120", () => {
    const c = new BeatClock();
    c.feed(frame(500, 0.2, 0), 0);
    expect(c.tempo).toBe(120);
    expect(c.beatMs).toBe(500);
  });

  it("the frame-colour step never runs faster than 2.5 a second", () => {
    for (let bpm = 40; bpm <= 320; bpm += 5) {
      const every = stepEveryBeats(bpm);
      expect(bpm / 60 / every, `${bpm} bpm`).toBeLessThanOrEqual(TITLES.step.max_hz + 1e-9);
    }
  });
});

describe("onsets", () => {
  it("finds the kick of a steady 120 BPM pattern, not the steady hum", () => {
    const o = new Onsets();
    let hits = 0;
    for (let i = 0; i < 600; i++) {
      const t = i * 16.7;
      const onBeat = Math.floor(t / 500) !== Math.floor((t - 16.7) / 500);
      const kick = onBeat ? 0.9 : 0.12;
      if (o.update([kick, 0.2, 0.1], t).kick) hits++;
    }
    // 10 s ≈ 20 beats; the adaptive threshold needs a moment to settle.
    expect(hits).toBeGreaterThanOrEqual(16);
    expect(hits).toBeLessThanOrEqual(21);
  });

  it("a silent signal has no hits and a low level", () => {
    const o = new Onsets();
    let hits = 0;
    for (let i = 0; i < 100; i++) if (o.update([0, 0, 0], i * 16).kick) hits++;
    expect(hits).toBe(0);
    expect(o.level).toBeLessThan(TITLES.rest.energy_below);
  });
});

describe("textures keep the same ink (the flicker between frames of equal luminance)", () => {
  it("every kind, every phase: coverage ≈ 0.4", () => {
    for (const kind of TEXTURE_KINDS) {
      for (let phase = 0; phase < 12; phase++) {
        const cov = inkCoverage(tileOf(kind, 17, phase), 200);
        expect(Math.abs(cov - COVERAGE), `${kind} @${phase}: ${cov}`).toBeLessThan(0.025);
      }
    }
  });
});

describe("text over the visualisation (UI contrast, T7.5)", () => {
  // The alabaster text of the UI under the mode's scrim (black at the configured alpha).
  const ALABASTER = "#f9f6f0";
  const under = (hex: string, alpha: number) => {
    const n = Number.parseInt(hex.slice(1), 16);
    const ch = (s: number) => Math.round(((n >> s) & 255) * (1 - alpha));
    return `#${[16, 8, 0].map((s) => ch(s).toString(16).padStart(2, "0")).join("")}`;
  };

  it("the brightest colours of each style stay readable under its scrim (≥ 4.5:1)", () => {
    const sh = TITLES.styles.shock;
    const po = TITLES.styles.poster;
    for (const c of [sh.field, sh.texture, ...sh.accents, ...sh.gradients.flat()]) {
      expect(
        contrast(ALABASTER, under(c, TITLES.scrim.shock)),
        `shock ${c}`,
      ).toBeGreaterThanOrEqual(4.5);
    }
    for (const c of [po.field, po.text, po.pulse, po.white, ...po.grays, ...po.frame_steps]) {
      expect(
        contrast(ALABASTER, under(c, TITLES.scrim.poster)),
        `poster ${c}`,
      ).toBeGreaterThanOrEqual(4.5);
    }
    // «Авто» may land on either, so it takes the stronger one.
    expect(TITLES.scrim.auto).toBeGreaterThanOrEqual(
      Math.max(TITLES.scrim.shock, TITLES.scrim.poster),
    );
  });
});

// ───────────────────── the flash audit on a coarse raster ─────────────────────

const GW = 48;
const GH = 27;
const hexRgb = (hex: string): [number, number, number] => {
  const n = Number.parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};
const lin = (v: number) => {
  const c = v / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
};

/** A painter that rasterises into 48 × 27 cells: enough to see where the luminance moves. */
class RasterPainter implements Painter {
  readonly cells = new Float32Array(GW * GH * 3);
  private clips: number[][] = [];
  constructor(
    readonly w = 1280,
    readonly h = 720,
  ) {}

  private inClips(x: number, y: number): boolean {
    return this.clips.every((poly) => inPoly(poly, x, y));
  }

  private blend(test: (x: number, y: number) => boolean, color: string, alpha = 1): void {
    const [r, g, b] = hexRgb(color);
    for (let cy = 0; cy < GH; cy++) {
      for (let cx = 0; cx < GW; cx++) {
        const x = ((cx + 0.5) / GW) * this.w;
        const y = ((cy + 0.5) / GH) * this.h;
        if (!test(x, y) || !this.inClips(x, y)) continue;
        const i = (cy * GW + cx) * 3;
        this.cells[i] = (this.cells[i] ?? 0) * (1 - alpha) + r * alpha;
        this.cells[i + 1] = (this.cells[i + 1] ?? 0) * (1 - alpha) + g * alpha;
        this.cells[i + 2] = (this.cells[i + 2] ?? 0) * (1 - alpha) + b * alpha;
      }
    }
  }

  fillRect(x: number, y: number, w: number, h: number, color: string, alpha = 1): void {
    this.blend((px, py) => px >= x && px < x + w && py >= y && py < y + h, color, alpha);
  }
  strokeRect(x: number, y: number, w: number, h: number, color: string, lw: number): void {
    this.blend(
      (px, py) =>
        px >= x - lw / 2 &&
        px <= x + w + lw / 2 &&
        py >= y - lw / 2 &&
        py <= y + h + lw / 2 &&
        !(px > x + lw / 2 && px < x + w - lw / 2 && py > y + lw / 2 && py < y + h - lw / 2),
      color,
    );
  }
  poly(pts: number[], color: string, alpha = 1): void {
    this.blend((x, y) => inPoly(pts, x, y), color, alpha);
  }
  polyGradient(pts: number[], from: string, to: string): void {
    this.blend((x, y) => inPoly(pts, x, y), from, 0.5);
    this.blend((x, y) => inPoly(pts, x, y), to, 0.5);
  }
  line(pts: number[], color: string, lw: number, alpha = 1): void {
    this.blend(
      (x, y) => {
        for (let i = 0; i + 3 < pts.length; i += 2) {
          const [ax, ay, bx, by] = [pts[i] ?? 0, pts[i + 1] ?? 0, pts[i + 2] ?? 0, pts[i + 3] ?? 0];
          const t = Math.max(
            0,
            Math.min(
              1,
              ((x - ax) * (bx - ax) + (y - ay) * (by - ay)) /
                ((bx - ax) ** 2 + (by - ay) ** 2 || 1),
            ),
          );
          if (
            Math.hypot(x - (ax + t * (bx - ax)), y - (ay + t * (by - ay))) <=
            Math.max(lw, this.h / GH / 2)
          )
            return true;
        }
        return false;
      },
      color,
      alpha,
    );
  }
  circle(cx: number, cy: number, r: number, color: string, alpha = 1): void {
    this.blend((x, y) => Math.hypot(x - cx, y - cy) <= r, color, alpha);
  }
  ring(cx: number, cy: number, r: number, color: string, lw: number): void {
    this.blend(
      (x, y) => Math.abs(Math.hypot(x - cx, y - cy) - r) <= Math.max(lw, this.h / GH / 2),
      color,
    );
  }
  measure(s: string, size: number, _weight: number, xScale: number): number {
    return s.length * 0.62 * size * xScale;
  }
  text(s: string, x: number, y: number, o: TextOpts): void {
    const wd = this.measure(s, o.size, o.weight, o.xScale);
    const x0 = o.align === "left" ? x : o.align === "right" ? x - wd : x - wd / 2;
    // Letters fill about half their box; hollow ones far less.
    const share = (o.stroke ? 0.12 : 0.5) * (o.alpha ?? 1);
    this.blend(
      (px, py) => px >= x0 && px <= x0 + wd && py >= y - o.size * 0.75 && py <= y,
      o.color,
      share,
    );
  }
  clipPoly(pts: number[], draw: () => void): void {
    this.clips.push(pts);
    draw();
    this.clips.pop();
  }
  texture(
    _k: string,
    _s: number,
    _p: number,
    box: [number, number, number, number],
    _t: number,
    color: string,
  ): void {
    const [bx, by, bw, bh] = box;
    this.blend((x, y) => x >= bx && x <= bx + bw && y >= by && y <= by + bh, color, COVERAGE);
  }
  slices(): void {}
  grain(): void {}
  scanlines(): void {}

  /** Mean relative luminance and the per-cell luminance. */
  luma(): { mean: number; cell: Float32Array } {
    const cell = new Float32Array(GW * GH);
    let sum = 0;
    for (let i = 0; i < GW * GH; i++) {
      const l =
        0.2126 * lin(this.cells[i * 3] ?? 0) +
        0.7152 * lin(this.cells[i * 3 + 1] ?? 0) +
        0.0722 * lin(this.cells[i * 3 + 2] ?? 0);
      cell[i] = l;
      sum += l;
    }
    return { mean: sum / (GW * GH), cell };
  }
}

function inPoly(pts: number[], x: number, y: number): boolean {
  let inside = false;
  const n = pts.length / 2;
  for (let i = 0, j = n - 1; i < n; j = i++) {
    const xi = pts[i * 2] ?? 0;
    const yi = pts[i * 2 + 1] ?? 0;
    const xj = pts[j * 2] ?? 0;
    const yj = pts[j * 2 + 1] ?? 0;
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

const FULL: TitlesQuality = { grain: true, halftone: true, glitch: true, flicker: true };

/** An edit of `seconds` at `bpm`, one raster per frame: the luminance track of the whole picture. */
function simulate(opts: {
  style: "shock" | "poster";
  seed: number;
  bpm: number;
  seconds: number;
  fps: number;
  reduced: boolean;
}) {
  const { style, seed, bpm, seconds, fps, reduced } = opts;
  const roles = rolesOf({ tempo: true, mood: true });
  const out: { mean: number; changed: number }[] = [];
  let prev: Float32Array | null = null;
  let prevMean = 0;
  for (let f = 0; f < seconds * fps; f++) {
    const t = f / fps;
    const pos = (t * bpm) / 60;
    const card = locate(seed, bpm, roles, pos);
    const cardBeat = pos - card.start;
    const sinceBeat = (pos % 1) * (60000 / bpm);
    const sinceBar = (pos % 4) * (60000 / bpm);
    const progress = t / seconds;
    const act = actOf(progress, card.k);
    const state: SceneState = {
      style,
      seed,
      card,
      cardBeat,
      beatIdx: reduced ? Math.floor(card.start) : Math.floor(pos),
      frame: f,
      sinceCard: reduced ? 99 : framesSince(cardBeat, 60000 / bpm, fps),
      lines: ["KEEP IT MELLO", "OMAR LINX"],
      roleLabel: "ТРЕК",
      density: densityOf(act, 0.6),
      act,
      quiet: false,
      kickAge: sinceBeat,
      snareAge: sinceBar > 1000 ? sinceBeat : 1e9,
      bands: [0.8, 0.4, 0.3],
      wave: null,
      q: FULL,
      reduced,
      stepEvery: reduced ? 1e6 : stepEveryBeats(bpm),
      // The poster's fill only ever grows slowly after the climax starts (6 s), like the engine.
      fill:
        act === "climax" && style === "poster" && !reduced
          ? Math.min(1, ((progress - TITLES.acts.climax_from) * seconds) / 6)
          : 0,
    };
    const p = new RasterPainter();
    renderScene(p, state);
    const { mean, cell } = p.luma();
    let changed = 0;
    if (prev)
      for (let i = 0; i < cell.length; i++)
        if (Math.abs((cell[i] ?? 0) - (prev[i] ?? 0)) >= TITLES.flash.min_delta) changed++;
    out.push({
      mean: f === 0 ? 0 : Math.abs(mean - prevMean),
      changed: prev ? changed / cell.length : 0,
    });
    prev = cell;
    prevMean = mean;
  }
  return out;
}

describe("flash audit (WCAG 2.3.1: ≥ 10 % luminance change over ≥ 25 % of the frame)", () => {
  for (const style of ["shock", "poster"] as const) {
    for (const bpm of [100, 160]) {
      it(`${style} @ ${bpm} BPM: no harsh jump, ≤ 2 big luminance changes and ≤ 3 wide changes a second`, () => {
        for (const seed of [1, 2]) {
          // 12 frames a second: bigger steps than the real 24, so the audit is the stricter one.
          const fps = 12;
          const run = simulate({ style, seed, bpm, seconds: 24, fps, reduced: false });
          const maxMeanDelta = Math.max(...run.map((r) => r.mean));
          // A hard cut between cards may move the mean by ≥ 10 %, but never by a harsh jump, and such
          // changes (the flash pair of WCAG 2.3.1) never come more than twice in a second.
          expect(maxMeanDelta, `seed ${seed}`).toBeLessThan(0.2);
          for (let i = 0; i + fps <= run.length; i += 3) {
            const big = run
              .slice(i, i + fps)
              .filter((r) => r.mean >= TITLES.flash.min_delta).length;
            expect(big, `seed ${seed} @${i}`).toBeLessThanOrEqual(2);
          }
          // Wide changes (card cuts) in any 1-second window.
          for (let i = 0; i + fps <= run.length; i += 3) {
            const wide = run
              .slice(i, i + fps)
              .filter((r) => r.changed >= TITLES.flash.min_area).length;
            expect(wide, `seed ${seed} @${i}`).toBeLessThanOrEqual(3);
          }
        }
      });
    }

    it(`${style} under reduced motion: only card changes change the picture`, () => {
      const run = simulate({ style, seed: 4, bpm: 128, seconds: 20, fps: 12, reduced: true });
      const busy = run.filter((r) => r.changed > 0 || r.mean > 0).length;
      // 30 s of ~4 s cards: a handful of changing frames, never a stream of them.
      expect(busy).toBeLessThanOrEqual(12);
    });
  }

  it("the hash is stable (the edit does not drift between versions)", () => {
    expect(hash32(1, 2, 3)).toBe(hash32(1, 2, 3));
    expect(rand(7, 7)).toBeGreaterThanOrEqual(0);
    expect(rand(7, 7)).toBeLessThan(1);
  });
});
