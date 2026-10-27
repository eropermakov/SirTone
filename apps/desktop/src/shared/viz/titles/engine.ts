import { tryFlash } from "../flashBudget";
import { BeatClock, type ClockFrame, Onsets, stepEveryBeats } from "./clock";
import { type LevelKey, levelKey, TITLES, type TitlesStylePref } from "./config";
import { CanvasPainter } from "./painter";
import {
  actOf,
  type Card,
  cardOf,
  DensityHold,
  framesSince,
  locate,
  type Mood,
  moodOf,
  type Role,
  resolveStyle,
  rolesOf,
  StylePick,
  type Vibe,
  wrapWords,
} from "./plan";
import { renderScene, type SceneState } from "./scenes";

/** What the engine writes on the cards: the track's own data and the words for the roles. */
export type TrackMeta = {
  seed: number;
  title: string | null;
  artist: string | null;
  vibe: Vibe | null;
  labels: {
    title: string;
    artist: string;
    tempo: string;
    mood: string;
    moods: Record<Mood, string>;
  };
};

export type TitlesStats = { frames: number; lastWorkMs: number; maxWorkMs: number };
export const titlesStats: TitlesStats = { frames: 0, lastWorkMs: 0, maxWorkMs: 0 };

/**
 * The «Титры» engine (FR-VIZ-012, D-122, `06` §7.8): an automatic editor, not a loudness meter.
 * The track's beat grid sets the cards (2–4 bars, 3–5 s); live sound only scales the events
 * already planned. Canvas 2D at 24 frames a second (12 on «Слабое») — the stepped, jerky motion
 * of the reference is also the cheap one. All time comes from the audio frames' `tAudioMs`.
 */
export class TitlesEngine {
  private ctx: CanvasRenderingContext2D | null = null;
  private painter: CanvasPainter | null = null;
  private raf = 0;
  private running = false;
  private nextDraw = 0;
  private fontsReady = false;
  private disposed = false;
  private clock = new BeatClock();
  private onsets = new Onsets();
  private meta: TrackMeta | null = null;
  private pref: TitlesStylePref = "auto";
  private level: LevelKey = "3";
  private bpmLock = 0;
  private cur: Card | null = null;
  private roleKey = "";
  private lines: { key: string; lines: string[]; label: string } | null = null;
  private progress = { posMs: 0, durMs: 0, at: 0 };
  private latest: { bands: [number, number, number]; mono: Uint8Array } | null = null;
  private quietSince = 0;
  private fill = { startedAt: -1 };
  private lastCardK = -1;
  private hold = new DensityHold();
  private pick = new StylePick();

  constructor(
    private canvas: HTMLCanvasElement,
    private opts: { reducedMotion: boolean; onFail?: () => void },
  ) {}

  /** `false` — no 2D context (the caller falls back to «Волны»). */
  init(): boolean {
    const ctx = this.canvas.getContext("2d", { alpha: false });
    if (!ctx) return false;
    this.ctx = ctx;
    // The first draw waits for the font: a canvas never falls back to Segoe for good.
    void Promise.all(
      ["600", "700", "800", "900"].map((w) => document.fonts.load(`${w} 32px Geologica`)),
    ).then(
      () => {
        this.fontsReady = true;
      },
      () => {
        this.fontsReady = true;
      },
    );
    this.resize();
    return true;
  }

  setTrack(meta: TrackMeta): void {
    const changed = !this.meta || this.meta.seed !== meta.seed || this.meta.title !== meta.title;
    this.meta = meta;
    if (changed) {
      this.clock.reset();
      this.onsets.reset();
      this.bpmLock = 0;
      this.cur = null;
      this.lines = null;
      this.fill.startedAt = -1;
      this.lastCardK = -1;
      this.hold.reset();
      this.pick.reset();
    }
  }

  setProgress(posMs: number, durMs: number | null): void {
    this.progress = { posMs, durMs: durMs ?? 0, at: performance.now() };
  }

  setStyle(pref: TitlesStylePref): void {
    this.pref = pref;
    this.pick.reset();
  }

  setQuality(level: number): void {
    const k = levelKey(level);
    if (k === this.level) return;
    this.level = k;
    this.resize();
  }

  setAudioFrame(f: ClockFrame & { bands: [number, number, number]; mono: Uint8Array }): void {
    const now = performance.now();
    this.clock.feed(f, now);
    if (f.playing) this.onsets.update(f.bands, now);
    this.latest = { bands: f.bands, mono: f.mono };
    if (f.bpm > 0 && this.bpmLock === 0) {
      this.bpmLock = f.bpm;
      this.cur = null;
    }
  }

  pause(): void {
    this.running = false;
    cancelAnimationFrame(this.raf);
  }

  resume(): void {
    if (this.running || this.disposed || !this.ctx) return;
    this.running = true;
    this.nextDraw = 0;
    this.raf = requestAnimationFrame(this.tick);
  }

  resize(): void {
    const cv = this.canvas;
    const scale = TITLES.pixel_scale[this.level];
    const w = Math.max(2, Math.round(cv.clientWidth * scale));
    const h = Math.max(2, Math.round(cv.clientHeight * scale));
    if (cv.width !== w || cv.height !== h) {
      cv.width = w;
      cv.height = h;
    }
    if (this.ctx) this.painter = new CanvasPainter(this.ctx, cv.width, cv.height);
  }

  dispose(): void {
    this.disposed = true;
    this.pause();
    this.ctx = null;
    this.painter = null;
  }

  /** Drawing rate: the level's, or a still 5 a second under reduced motion (only cards change). */
  private fps(): number {
    return this.opts.reducedMotion ? 5 : TITLES.fps[this.level];
  }

  /** The scene state at `now` (also what the diagnostics and the tests read). */
  state(now: number): SceneState | null {
    const meta = this.meta;
    const p = this.painter;
    if (!meta || !p) return null;
    const fps = this.fps();
    const q = TITLES.quality[this.level];
    const bpm = this.bpmLock > 0 ? this.bpmLock : 0;
    const pos = this.clock.pos(now);
    const tempo = bpm > 0 ? bpm : TITLES.card.fallback_bpm;
    const roles: Role[] = rolesOf({ tempo: bpm > 0, mood: moodOf(meta.vibe) !== null });
    const key = `${bpm}|${roles.join()}`;
    if (key !== this.roleKey) {
      this.roleKey = key;
      this.cur = null;
    }
    // The card: advance from the current one, or walk from the start after a seek / reset.
    let card = this.cur;
    if (card && pos >= card.start && pos < card.start + card.beats) {
      // still in it
    } else if (card && pos >= card.start + card.beats && pos < card.start + card.beats * 3) {
      let c: Card = card;
      while (pos >= c.start + c.beats)
        c = cardOf(meta.seed, c.k + 1, c.start + c.beats, tempo, roles);
      card = c;
    } else {
      card = locate(meta.seed, tempo, roles, pos);
    }
    this.cur = card;
    if (card.k !== this.lastCardK) {
      this.lastCardK = card.k;
      this.lines = null;
      if (this.opts.reducedMotion) this.fade();
    }

    const want = resolveStyle(this.pref, meta.seed, bpm, meta.vibe);
    const style = this.pick.update(card.k, want, this.pref !== "auto", () => tryFlash(now));
    const progress =
      this.progress.durMs > 0
        ? Math.min(
            1,
            (this.progress.posMs + (this.clock.isPlaying ? now - this.progress.at : 0)) /
              this.progress.durMs,
          )
        : null;
    const act = actOf(progress, card.k);
    const fed = this.clock.fed(now);
    const level = fed ? this.onsets.level : 0.3;
    // Silence is part of the edit: an empty field and one small form while the music is thin.
    if (!fed || !this.clock.isPlaying || level >= TITLES.rest.energy_below) this.quietSince = 0;
    else if (this.quietSince === 0) this.quietSince = now;
    const quiet = this.quietSince > 0 && now - this.quietSince > TITLES.rest.hold_ms;

    if (
      style === "poster" &&
      act === "climax" &&
      !this.opts.reducedMotion &&
      this.fill.startedAt < 0 &&
      tryFlash(now)
    ) {
      this.fill.startedAt = now;
    }
    const fill = this.fill.startedAt < 0 ? 0 : Math.min(1, (now - this.fill.startedAt) / 6000);

    const lines = this.textOf(card, style, tempo, p.w, p.h, meta);
    const beatMs = this.clock.beatMs;
    const cardBeat = pos - card.start;
    return {
      style,
      seed: meta.seed,
      card,
      cardBeat,
      // Reduced motion: nothing moves between cards, the beat index stays the card's first.
      beatIdx: this.opts.reducedMotion ? Math.floor(card.start) : Math.floor(pos),
      frame: Math.floor(now / (1000 / fps)),
      sinceCard: this.opts.reducedMotion ? 99 : framesSince(cardBeat, beatMs, fps),
      lines: lines.lines,
      roleLabel: lines.label,
      // Structure once per card; the live loudness only drives continuous amplitudes.
      density: this.hold.update(card.k, act, level),
      energy: level,
      act,
      quiet,
      kickAge: now - this.onsets.kickAt,
      snareAge: now - this.onsets.snareAt,
      bands: this.latest?.bands ?? [0, 0, 0],
      wave: this.latest?.mono ?? null,
      q,
      reduced: this.opts.reducedMotion,
      stepEvery: this.opts.reducedMotion ? 1e6 : stepEveryBeats(tempo),
      fill,
    };
  }

  private textOf(
    card: Card,
    style: "shock" | "poster",
    bpm: number,
    w: number,
    h: number,
    meta: TrackMeta,
  ) {
    const cacheKey = `${card.k}|${style}|${w}|${meta.title}|${meta.artist}`;
    if (this.lines?.key === cacheKey) return this.lines;
    let text = "";
    let label = "";
    switch (card.role) {
      case "title":
        text = meta.title ?? "";
        label = meta.labels.title;
        break;
      case "artist":
        text = meta.artist ?? "";
        label = meta.labels.artist;
        break;
      case "tempo":
        text = String(Math.round(bpm));
        label = meta.labels.tempo;
        break;
      case "mood": {
        const m = moodOf(meta.vibe);
        text = m ? meta.labels.moods[m] : "";
        label = meta.labels.mood;
        break;
      }
    }
    const p = this.painter;
    const shock = style === "shock";
    const xs = shock ? TITLES.text.xscale_wide : TITLES.text.xscale_narrow;
    const nominal = (shock ? 0.2 : 0.34) * h;
    const maxW = (shock ? 0.86 : 0.88) * w;
    const upper = text.toLocaleUpperCase("ru");
    const lines = p
      ? wrapWords(
          upper,
          TITLES.text.max_lines,
          maxW / nominal,
          (s) => p.measure(s, 100, shock ? 900 : 800, xs) / 100,
        )
      : [];
    this.lines = { key: cacheKey, lines, label };
    return this.lines;
  }

  /** Reduced motion: a card change is a short fade, nothing more (D-122). */
  private fade(): void {
    const s = this.canvas.style;
    s.transition = "none";
    s.opacity = "0.35";
    requestAnimationFrame(() => {
      s.transition = "opacity 200ms";
      s.opacity = "1";
    });
  }

  private tick = (now: number): void => {
    if (!this.running) return;
    this.raf = requestAnimationFrame(this.tick);
    const gap = 1000 / this.fps();
    if (now < this.nextDraw) return;
    this.nextDraw = now - this.nextDraw > gap * 2 ? now + gap : (this.nextDraw || now) + gap;
    if (!this.fontsReady || !this.painter) return;
    const t0 = performance.now();
    const s = this.state(t0);
    if (s) renderScene(this.painter, s);
    const work = performance.now() - t0;
    titlesStats.frames += 1;
    titlesStats.lastWorkMs = work;
    titlesStats.maxWorkMs = Math.max(titlesStats.maxWorkMs, work);
  };

  /** One frame now (tests and the still frame of reduced motion). */
  drawNow(now = performance.now()): void {
    if (!this.painter) return;
    const s = this.state(now);
    if (s) renderScene(this.painter, s);
  }
}
