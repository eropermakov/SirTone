/**
 * Deterministic randomness for the edit (D-122): the same track always gets the same «opening»,
 * like a cut that was made once. `rand(seed, k, …)` is a pure hash of its arguments — no state,
 * so a seek or a restart lands on the same card.
 */
export function hash32(...n: number[]): number {
  let h = 0x811c9dc5;
  for (const v of n) {
    let x = Math.floor(v) | 0;
    x = Math.imul(x ^ (x >>> 16), 0x45d9f3b);
    x = Math.imul(x ^ (x >>> 16), 0x45d9f3b);
    x ^= x >>> 16;
    h = Math.imul(h ^ x, 0x01000193);
    h ^= h >>> 15;
  }
  h = Math.imul(h ^ (h >>> 13), 0x5bd1e995);
  return (h ^ (h >>> 15)) >>> 0;
}

/** 0 ≤ x < 1. */
export const rand = (...n: number[]): number => hash32(...n) / 4294967296;

export const pick = <T>(arr: readonly T[], ...n: number[]): T =>
  arr[Math.floor(rand(...n) * arr.length) % arr.length] as T;

/** A stable seed from text (a track id missing — the file name still gives one). */
export function seedOf(trackId: number | null, title: string | null): number {
  if (trackId !== null) return hash32(trackId, 0x7a11);
  let h = 7;
  for (const ch of title ?? "") h = hash32(h, ch.charCodeAt(0));
  return h;
}
