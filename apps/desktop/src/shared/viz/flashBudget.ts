/**
 * One flash budget for the whole screen (WCAG 2.3.1, `CLAUDE.md`: flashes ≤ 1 in 10 s): the
 * «Волны» like/click lift and the storm's flashes (T4.20) ask here, so together they never
 * flash twice within the gap. `now` — `performance.now()`.
 */
export const FLASH_GAP_MS = 10_000;

let last = Number.NEGATIVE_INFINITY;

/** `true` — flash now (and the gap starts); `false` — too soon after the last one. */
export function tryFlash(now: number, gapMs = FLASH_GAP_MS): boolean {
  if (now - last < gapMs) return false;
  last = now;
  return true;
}

/** Tests only. */
export function resetFlashBudget(): void {
  last = Number.NEGATIVE_INFINITY;
}
