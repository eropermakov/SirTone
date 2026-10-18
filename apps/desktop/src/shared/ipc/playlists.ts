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

export const onPlaylistsChanged = async (cb: (id: number) => void): Promise<() => void> =>
  isTauri() ? events.playlistsChanged.listen((e) => cb(e.payload.id)) : () => {};
