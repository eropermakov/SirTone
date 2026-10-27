import { type StyleId, TITLES, type TitlesStylePref } from "./config";
import { hash32, rand } from "./rng";

/**
 * The cut list of the «Титры» edit (D-122, `06` §7.8): pure functions of (seed, tempo, position),
 * so the same track gets the same «opening» every time and a seek lands on the same card.
 * Levels of time, as in the reference (`01a` §B.5): section → act; phrase → card; strong beat →
 * entry / layout jump; beat → colour step; sub-beat → flicker; hit → doubling, slices, spike.
 */
export type Role = "title" | "artist" | "tempo" | "mood";
export type Variant = "fill" | "outline" | "echo" | "split";
export type Act = "prologue" | "ramp" | "climax";

export type Card = {
  k: number;
  /** Start, in beats from the track's start. */
  start: number;
  beats: number;
  role: Role;
  variant: Variant;
  /** Which scene template (0…2) of the style. */
  template: number;
  /** Titles justified to the right instead of the left. */
  flip: boolean;
  /** Entry of the title takes this many frames (2…4). */
  enterFrames: number;
  /** An offset into the palette steps, so neighbouring cards differ. */
  hue: number;
};

const BAR = () => TITLES.card.beats_per_bar;

/**
 * Beats per card so that it lasts 3–5 s at any tempo: whole bars where they fit (2–4 bars at the
 * usual tempos), else the nearest even number of beats (a slow 4/4 at 90 BPM: 6 beats).
 */
export function beatsFor(bpm: number, seed: number, k: number): number {
  const tempo = bpm > 0 ? bpm : TITLES.card.fallback_bpm;
  const beatS = 60 / tempo;
  const { min_s, max_s } = TITLES.card;
  const fits = (n: number) => n * beatS >= min_s && n * beatS <= max_s;
  const bar = BAR();
  const whole = [1, 2, 3, 4].map((n) => n * bar).filter(fits);
  const pool = whole.length > 0 ? whole : [2, 4, 6, 8, 10, 12, 14, 16].filter(fits);
  if (pool.length > 0) return pool[Math.floor(rand(seed, k, 0xba5) * pool.length)] as number;
  // Out of any sensible range: the closest to the middle.
  const mid = (min_s + max_s) / 2;
  return Math.max(1, Math.round(mid / beatS));
}

/** The roles available for a track, in the order the cards use them. */
export function rolesOf(has: { tempo: boolean; mood: boolean }): Role[] {
  const r: Role[] = ["title", "artist"];
  if (has.tempo) r.push("tempo");
  if (has.mood) r.push("mood");
  return r;
}

/** Even cards use the quiet pair, odd ones the loud pair, so no variant ever repeats in a row. */
const VARIANTS: [Variant, Variant][] = [
  ["fill", "echo"],
  ["outline", "split"],
];

export function cardOf(seed: number, k: number, start: number, bpm: number, roles: Role[]): Card {
  const first = k === 0;
  // The first cards are the track's own title and artist; later ones cycle through all roles.
  const role: Role = first
    ? "title"
    : k === 1
      ? "artist"
      : (roles[(k + (hash32(seed, 0x701e) % roles.length)) % roles.length] ?? "title");
  const lo = TITLES.card.enter_frames[0];
  const hi = TITLES.card.enter_frames[1];
  return {
    k,
    start,
    beats: beatsFor(bpm, seed, k),
    role,
    // The prologue card is plain; the rest vary, no two the same in a row.
    variant: first
      ? "fill"
      : ((VARIANTS[k % 2] as [Variant, Variant])[hash32(seed, k, 0xfa) % 2] as Variant),
    template: hash32(seed, k, 0x7e) % 3,
    flip: rand(seed, k, 0xf1) < 0.5,
    enterFrames: lo + (hash32(seed, k, 0xe7) % (hi - lo + 1)),
    hue: hash32(seed, k, 0x1e) % 3,
  };
}

/** The card the position (in beats) is in; walks the cards from the start (≈ 60 in a long track). */
export function locate(seed: number, bpm: number, roles: Role[], pos: number): Card {
  let start = 0;
  for (let k = 0; k < 4096; k++) {
    const c = cardOf(seed, k, start, bpm, roles);
    if (pos < start + c.beats) return c;
    start += c.beats;
  }
  return cardOf(seed, 4096, start, bpm, roles);
}

/** The act from the place in the track; unknown length: the first two cards are the prologue. */
export function actOf(progress: number | null, k: number): Act {
  if (progress === null) return k < 2 ? "prologue" : "ramp";
  if (progress < TITLES.acts.prologue_until) return "prologue";
  if (progress >= TITLES.acts.climax_from) return "climax";
  return "ramp";
}

/** How busy the frame is, 0…1: the act's base plus the loudness (D-122: dynamics one to one). */
export function densityOf(act: Act, level: number): number {
  const d = TITLES.acts.density[act] + TITLES.acts.energy_gain * level;
  return Math.min(1, Math.max(0, d));
}

export type Vibe = { valence: number | null; arousal: number | null };

/** The style: «Авто» follows the arousal of the track, else its tempo, else the seed. */
export function resolveStyle(
  pref: TitlesStylePref,
  seed: number,
  bpm: number,
  vibe: Vibe | null,
): StyleId {
  if (pref !== "auto") return pref;
  if (vibe?.arousal != null) return vibe.arousal >= TITLES.auto.arousal_split ? "shock" : "poster";
  if (bpm > 0) return bpm >= TITLES.auto.bpm_split ? "shock" : "poster";
  return hash32(seed, 0x57) % 2 === 0 ? "shock" : "poster";
}

export type Mood = "happy" | "energetic" | "calm" | "sad";

/** The word for «Настроение», from the vibe vector (the keys are `viz.moods.*`). */
export function moodOf(vibe: Vibe | null): Mood | null {
  if (!vibe || vibe.valence == null || vibe.arousal == null) return null;
  if (vibe.valence < 0.4) return "sad";
  if (vibe.arousal >= 0.6) return "energetic";
  if (vibe.valence >= 0.6) return "happy";
  return "calm";
}

/**
 * Words broken into at most `maxLines` lines, greedily, each no wider than `maxWidth` at the
 * size 1 (`widthOf` measures a string at that size). A single word wider than the line stays whole
 * — the caller shrinks or crops it.
 */
export function wrapWords(
  text: string,
  maxLines: number,
  maxWidth: number,
  widthOf: (s: string) => number,
): string[] {
  const words = text.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  const lines: string[] = [];
  let cur = "";
  for (const w of words) {
    const next = cur ? `${cur} ${w}` : w;
    if (cur && widthOf(next) > maxWidth && lines.length < maxLines - 1) {
      lines.push(cur);
      cur = w;
    } else {
      cur = next;
    }
  }
  if (cur) lines.push(cur);
  return lines;
}

/** Frames since a card started (quantised to the grid of `fps`). */
export function framesSince(cardBeat: number, beatMs: number, fps: number): number {
  return Math.floor((Math.max(0, cardBeat) * beatMs) / (1000 / fps));
}

/**
 * The structure of a card (how many cut-outs, whether the accent shape and scanlines show) is
 * decided **once per card** from the previous card's mean loudness, never frame by frame: loudness
 * hovering near a threshold must not toggle whole layers at beat rate (a flash path, WCAG 2.3.1).
 * The live level stays for continuous amplitudes only (D-122).
 */
export class DensityHold {
  private k = -1;
  private density = 0;
  private sum = 0;
  private n = 0;

  reset(): void {
    this.k = -1;
    this.density = 0;
    this.sum = 0;
    this.n = 0;
  }

  /** Call every frame with the live `level`; the result only changes when the card does. */
  update(k: number, act: Act, level: number): number {
    if (k !== this.k) {
      const prev = this.n > 0 ? this.sum / this.n : level;
      this.k = k;
      this.density = densityOf(act, prev);
      this.sum = 0;
      this.n = 0;
    }
    this.sum += level;
    this.n += 1;
    return this.density;
  }
}

/**
 * The style as shown: «Авто» may change its mind while the track's data arrives (the tempo, then
 * the vibe), but the whole field swapping yellow ↔ grey is a flash. So an automatic pick changes
 * at most once per track, only at a card change and only when `allow()` (the shared flash budget)
 * says yes; a manual choice applies at once (the person's own action).
 */
export class StylePick {
  private cur: StyleId | null = null;
  private lastK = -1;
  private swaps = 0;

  reset(): void {
    this.cur = null;
    this.lastK = -1;
    this.swaps = 0;
  }

  update(k: number, want: StyleId, manual: boolean, allow: () => boolean): StyleId {
    const cardChanged = k !== this.lastK;
    this.lastK = k;
    if (this.cur === null || manual) {
      this.cur = want;
    } else if (cardChanged && want !== this.cur && this.swaps < 1 && allow()) {
      this.cur = want;
      this.swaps += 1;
    }
    return this.cur;
  }
}
