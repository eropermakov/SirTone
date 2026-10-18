import { isTauri } from "@tauri-apps/api/core";
import { commands, events, type PlaylistEntry, type PlaylistItem } from "./bindings";

export type { PlaylistEntry, PlaylistItem };

/**
 * Manual playlists (T2.11, FR-PLS-001/004). Outside Tauri (browser preview, Playwright) there
 * are none. Items are addressed by position — a track may be in a playlist twice.
 */
const unwrap = <T>(r: { status: "ok"; data: T } | { status: "error"; error: unknown }): T => {
  if (r.status === "error") throw r.error;
  return r.data;
};

export const playlistsList = async (): Promise<PlaylistItem[]> =>
  isTauri() ? unwrap(await commands.playlistsList()) : [];

export const createPlaylist = async (name: string): Promise<number> => {
  if (!isTauri()) throw new Error("no library outside Tauri");
  return unwrap(await commands.playlistCreate(name));
};

export const renamePlaylist = async (id: number, name: string): Promise<void> => {
  if (isTauri()) unwrap(await commands.playlistRename(id, name));
};

export const deletePlaylist = async (id: number): Promise<void> => {
  if (isTauri()) unwrap(await commands.playlistDelete(id));
};

export const playlistTracks = async (id: number): Promise<PlaylistEntry[]> =>
  isTauri() ? unwrap(await commands.playlistTracks(id)) : [];

/** Which of `tracks` the playlist already has («Уже в плейлисте»). */
export const alreadyIn = async (id: number, tracks: number[]): Promise<number[]> =>
  isTauri() ? unwrap(await commands.playlistAlreadyIn(id, tracks)) : [];

export const addToPlaylist = async (id: number, tracks: number[]): Promise<number> =>
  isTauri() ? unwrap(await commands.playlistAdd(id, tracks)) : 0;

/** «Убрать из плейлиста»: the tracks stay in the library. */
export const removeFromPlaylist = async (id: number, positions: number[]): Promise<void> => {
  if (isTauri()) unwrap(await commands.playlistRemove(id, positions));
};

export const movePlaylistItem = async (id: number, from: number, to: number): Promise<void> => {
  if (isTauri()) unwrap(await commands.playlistMove(id, from, to));
};

/** Smart playlists (T3.13): rules are the `05` §8.1 JSON; Rust validates them. */
export const smartCreate = async (name: string, rules: string): Promise<number> => {
  if (!isTauri()) throw new Error("no library outside Tauri");
  return unwrap(await commands.smartCreate(name, rules));
};

export const smartSetRules = async (id: number, rules: string): Promise<void> => {
  if (isTauri()) unwrap(await commands.smartSetRules(id, rules));
};

export const smartRules = async (id: number): Promise<string | null> =>
  isTauri() ? unwrap(await commands.smartRules(id)) : null;

/** «Подходит N треков»; rejects bad rules. */
export const smartPreview = async (rules: string): Promise<number> =>
  isTauri() ? unwrap(await commands.smartPreview(rules)) : 0;

export const smartRefresh = async (id: number): Promise<void> => {
  if (isTauri()) unwrap(await commands.smartRefresh(id));
};

export const smartSaveAsManual = async (id: number, name: string): Promise<number> => {
  if (!isTauri()) throw new Error("no library outside Tauri");
  return unwrap(await commands.smartSaveAsManual(id, name));
};

export const onPlaylistsChanged = async (cb: (id: number) => void): Promise<() => void> =>
  isTauri() ? events.playlistsChanged.listen((e) => cb(e.payload.id)) : () => {};
