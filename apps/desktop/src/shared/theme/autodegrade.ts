/**
 * Auto-degrade detector (T4.16, FR-PERF-004, the single rule D-47, `06` §6.2): the p95 frame time
 * over the last second, checked every 250 ms, above 20 ms **continuously for 3 s** → lower the
 * level by one. The first 2 s after a screen, visualiser-mode or theme change do not count; at
 * least 10 s between two degrades. Frame gaps over 250 ms (a hidden or stalled window) are
 * pauses, not frames. Pure: the caller feeds frame timestamps and asks `check`.
 */
export type Rule = {
  limitMs: number;
  windowMs: number;
  sustainMs: number;
  graceMs: number;
  cooldownMs: number;
  stepMs: number;
  gapMs: number;
};

export const D47: Rule = {
  limitMs: 20,
  windowMs: 1000,
  sustainMs: 3000,
  graceMs: 2000,
  cooldownMs: 10_000,
  stepMs: 250,
  gapMs: 250,
};

export class FrameWatch {
  private frames: { t: number; d: number }[] = [];
  private last = 0;
  private overSince: number | null = null;
  private quietUntil = 0;
  private lastDegrade = -Infinity;
  private lastCheck = 0;

  constructor(private rule: Rule = D47) {}

  /** A screen, mode or theme change: the next `graceMs` do not count. */
  reset(now: number): void {
    this.frames = [];
    this.last = 0;
    this.overSince = null;
    this.quietUntil = now + this.rule.graceMs;
  }

  frame(now: number): void {
    if (this.last > 0) {
      const d = now - this.last;
      if (d <= this.rule.gapMs) this.frames.push({ t: now, d });
      else this.overSince = null; // a pause, not a slow frame
    }
    this.last = now;
    const from = now - this.rule.windowMs;
    while (this.frames.length > 0 && (this.frames[0]?.t ?? 0) < from) this.frames.shift();
  }

  /** The p95 frame time of the window (0 — not enough frames). */
  p95(): number {
    if (this.frames.length < 10) return 0;
    const d = this.frames.map((f) => f.d).sort((a, b) => a - b);
    return d[Math.min(d.length - 1, Math.floor(d.length * 0.95))] ?? 0;
  }

  /** `true` — lower the level now (then the cooldown starts). */
  check(now: number): boolean {
    if (now - this.lastCheck < this.rule.stepMs) return false;
    this.lastCheck = now;
    if (now < this.quietUntil) {
      this.overSince = null;
      return false;
    }
    if (this.p95() > this.rule.limitMs) {
      this.overSince ??= now;
      if (
        now - this.overSince >= this.rule.sustainMs &&
        now - this.lastDegrade >= this.rule.cooldownMs
      ) {
        this.lastDegrade = now;
        this.overSince = null;
        return true;
      }
    } else {
      this.overSince = null;
    }
    return false;
  }
}
