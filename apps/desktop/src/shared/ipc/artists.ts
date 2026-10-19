import { isTauri } from "@tauri-apps/api/core";
import { type ArtistSuggestion, commands, type MergeDone } from "./bindings";

export type { ArtistSuggestion, MergeDone };

/** Artist merge (T3.12, FR-ART-003/004): suggestions, merge, undo within the session. */
const unwrap = <T>(r: { status: "ok"; data: T } | { status: "error"; error: unknown }): T => {
  if (r.status === "error") throw r.error;
  return r.data;
};

export const artistSuggestions = async (): Promise<ArtistSuggestion[]> =>
  isTauri() ? unwrap(await commands.artistSuggestions()) : [];

export const dismissSuggestion = async (a: number, b: number): Promise<void> => {
  if (isTauri()) unwrap(await commands.artistSuggestionDismiss(a, b));
};

/** `drop` joins `keep` (tracks, albums, ♡, its name becomes an alias). */
export const mergeArtists = async (keep: number, drop: number): Promise<MergeDone> => {
  if (!isTauri()) throw new Error("no library outside Tauri");
  return unwrap(await commands.artistsMerge(keep, drop));
};

export const undoMerge = async (token: number): Promise<number> =>
  isTauri() ? unwrap(await commands.artistsMergeUndo(token)) : 0;
