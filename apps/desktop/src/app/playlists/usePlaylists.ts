import { useSyncExternalStore } from "react";
import { onLibraryChanged } from "@/shared/ipc/library";
import { onPlaylistsChanged, type PlaylistItem, playlistsList } from "@/shared/ipc/playlists";

/**
 * One shared list of playlists for the sidebar and every «В плейлист ›» menu: loaded once,
 * refreshed on `PlaylistsChanged` and `LibraryChanged` (a removed track leaves playlists).
 */
let list: PlaylistItem[] = [];
const subs = new Set<() => void>();
let started = false;

export const refreshPlaylists = () =>
  playlistsList()
    .then((l) => {
      list = l;
      for (const s of subs) s();
    })
    .catch(() => {});

function start() {
  if (started) return;
  started = true;
  void refreshPlaylists();
  void onPlaylistsChanged(() => void refreshPlaylists());
  void onLibraryChanged(() => void refreshPlaylists());
}

export function usePlaylists(): PlaylistItem[] {
  start();
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => list,
  );
}

/** «Мой плейлист N» — the next free number (Spotify «My Playlist #N», `01b` F-13). */
export function nextName(lists: PlaylistItem[], make: (n: number) => string): string {
  const taken = new Set(lists.map((p) => p.name));
  let n = 1;
  while (taken.has(make(n))) n++;
  return make(n);
}
