import { isTauri } from "@tauri-apps/api/core";
import { commands, type DuplicatesView, type DupPair, type DupSide, events } from "./bindings";

export type { DuplicatesView, DupPair, DupSide };

/**
 * Duplicates by sound (T3.5, FR-DUP-001…004): pairs found after the background analysis, the
 * exact copies imports skipped, and the decisions. Outside Tauri there are none.
 */
const unwrap = <T>(r: { status: "ok"; data: T } | { status: "error"; error: unknown }): T => {
  if (r.status === "error") throw r.error;
  return r.data;
};

export const duplicatesList = async (): Promise<DuplicatesView> =>
  isTauri() ? unwrap(await commands.duplicatesList()) : { pairs: [], exact: [] };

export const duplicatesCount = async (): Promise<number> =>
  isTauri() ? unwrap(await commands.duplicatesCount()) : 0;

/** «Оставить оба» — the pair is not offered again. */
export const keepBoth = async (a: number, b: number): Promise<void> => {
  if (isTauri()) unwrap(await commands.duplicateKeepBoth(a, b));
};

/** `keep` takes `drop`'s plays, ♡, tags and playlist places; `drop`'s file → Recycle Bin. */
export const mergeDuplicate = async (keep: number, drop: number): Promise<void> => {
  if (isTauri()) unwrap(await commands.duplicateMerge(keep, drop));
};

/** Every «тот же звук» pair keeps the better file; returns how many files went. */
export const keepBest = async (): Promise<number> =>
  isTauri() ? unwrap(await commands.duplicatesKeepBest()) : 0;

export const onDuplicatesFound = async (cb: (pending: number) => void): Promise<() => void> =>
  isTauri() ? events.duplicatesFound.listen((e) => cb(e.payload.pending)) : () => {};
