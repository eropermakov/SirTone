import { TITLES } from "./config";

/** What the clock needs from a visualiser frame (`shared/ipc/viz.ts`). */
export type ClockFrame = { tAudioMs: number; beatPhase: number; bpm: number; playing: boolean };

/**
 * The beat clock of the edit (D-122, `06` §7.8). The source of time is the audio position of the
 * frames (`tAudioMs`, already shifted by the output latency, FR-VIZ-007) and the beat grid's
 * phase; the count of beats comes from the wraps of the phase. A seek (the audio time jumps) is
 * re-read from the time; a pause holds the position; `bpm = 0` (not analysed) or no frames at all
 * (the browser preview) fall back to a free-running 120 BPM. `pos` is monotonic between resets.
 */
export class BeatClock {
  /** Whole beats counted since the track's start, the phase in 0…1 of the current one. */
  private idx = 0;
  private phase = 0;
  private bpm = 0;
  private playing = false;
  private lastT = -1;
  private lastWall = 0;
  private seeded = false;
  private freeFrom = 0;
  private freePos = 0;
  private freeActive = false;
  private best = 0;

  reset(): void {
    this.idx = 0;
    this.phase = 0;
    this.lastT = -1;
    this.seeded = false;
    this.best = 0;
    this.freePos = 0;
    this.freeActive = false;
  }

  /** The tempo in use (the analysed one, or the fallback). */
  get tempo(): number {
    return this.bpm > 0 ? this.bpm : TITLES.card.fallback_bpm;
  }

  get beatMs(): number {
    return 60000 / this.tempo;
  }

  get isPlaying(): boolean {
    return this.playing;
  }

  feed(f: ClockFrame, wallMs: number): void {
    const jumped =
      this.lastT >= 0 && (f.tAudioMs < this.lastT - 80 || f.tAudioMs - this.lastT > 1500);
    this.playing = f.playing;
    this.bpm = f.bpm > 0 ? f.bpm : 0;
    if (!this.seeded || jumped) {
      // Absolute position from the time: the same moment always lands in the same bar.
      this.idx = Math.floor((f.tAudioMs * this.tempo) / 60000);
      this.seeded = true;
      this.best = 0;
    } else if (this.phase > 0.6 && f.beatPhase < 0.4) {
      this.idx += 1;
    }
    this.phase = f.beatPhase;
    this.lastT = f.tAudioMs;
    this.lastWall = wallMs;
  }

  /** Whether real frames drive the clock now (not older than 250 ms). */
  fed(wallMs: number): boolean {
    return this.seeded && wallMs - this.lastWall < 250;
  }

  /** The position in beats at the wall time; monotonic until `reset` or a seek. */
  pos(wallMs: number): number {
    let p: number;
    if (this.fed(wallMs)) {
      this.freeActive = false;
      const ahead = this.playing ? (wallMs - this.lastWall) / this.beatMs : 0;
      p = this.idx + this.phase + Math.min(0.25, ahead);
    } else {
      // No frames: a free clock at the fallback tempo; a paused one stands still.
      if (!this.freeActive) {
        this.freeActive = true;
        this.freePos = this.best;
        this.freeFrom = wallMs;
      }
      p =
        this.freePos + (this.playing || !this.seeded ? (wallMs - this.freeFrom) / this.beatMs : 0);
    }
    this.best = Math.max(this.best, p);
    return this.best;
  }
}

export type Hits = { kick: boolean; snare: boolean; hat: boolean };

/**
 * Onsets from the live band energies (D-122: the sound sets the strength of events that are
 * already planned, it never plans them). An adaptive threshold per band: the band must exceed its
 * running mean by `ratio`, rise by `rise` since the last frame and wait `min_gap_ms`.
 */
export class Onsets {
  private mean: [number, number, number] = [0, 0, 0];
  private prev: [number, number, number] = [0, 0, 0];
  private lastHit: [number, number, number] = [-1e9, -1e9, -1e9];
  /** Smoothed loudness 0…1: the density of events follows it. */
  level = 0;
  kickAt = -1e9;
  snareAt = -1e9;
  hatAt = -1e9;

  reset(): void {
    this.mean = [0, 0, 0];
    this.prev = [0, 0, 0];
    this.lastHit = [-1e9, -1e9, -1e9];
    this.level = 0;
    this.kickAt = this.snareAt = this.hatAt = -1e9;
  }

  update(bands: [number, number, number], wallMs: number): Hits {
    const o = TITLES.onset;
    const out: boolean[] = [false, false, false];
    for (let i = 0; i < 3; i++) {
      const x = bands[i] ?? 0;
      const m = this.mean[i] ?? 0;
      const hit =
        x > m * o.ratio + o.floor &&
        x - (this.prev[i] ?? 0) > o.rise &&
        wallMs - (this.lastHit[i] ?? 0) >= o.min_gap_ms;
      if (hit) this.lastHit[i] = wallMs;
      out[i] = hit;
      this.mean[i] = m * o.ema + x * (1 - o.ema);
      this.prev[i] = x;
    }
    const raw = (bands[0] ?? 0) * 0.5 + (bands[1] ?? 0) * 0.35 + (bands[2] ?? 0) * 0.15;
    this.level += (raw - this.level) * (raw > this.level ? 0.25 : 0.04);
    if (out[0]) this.kickAt = wallMs;
    if (out[1]) this.snareAt = wallMs;
    if (out[2]) this.hatAt = wallMs;
    return { kick: !!out[0], snare: !!out[1], hat: !!out[2] };
  }
}

/** The frame-colour step must not run faster than `max_hz` (WCAG 2.3.1): every n-th beat. */
export function stepEveryBeats(bpm: number, maxHz = TITLES.step.max_hz): number {
  const hz = (bpm > 0 ? bpm : TITLES.card.fallback_bpm) / 60;
  return Math.max(1, Math.ceil(hz / maxHz));
}
