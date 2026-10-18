import { isTauri } from "@tauri-apps/api/core";
import { commands, type SearchResults } from "./bindings";

export type { SearchResults };

const EMPTY: SearchResults = { best: null, tracks: [], artists: [], albums: [], playlists: [] };

/** Lexical search (T2.13): «лучшее совпадение» + groups. Outside Tauri — nothing. */
export const search = async (query: string): Promise<SearchResults> => {
  if (!isTauri()) return EMPTY;
  const r = await commands.librarySearch(query);
  if (r.status === "error") throw r.error;
  return r.data;
};
