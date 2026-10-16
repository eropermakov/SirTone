import { isTauri } from "@tauri-apps/api/core";
import {
  type AlbumPage,
  type ArtistPage,
  commands,
  events,
  type ImportFinished,
  type ImportProgress,
  type LibraryStatus,
  type PageCursor,
  type TrackItem,
} from "./bindings";

/**
 * Library IPC (Stage 1): folder, import, collection lists, playing tracks. Generated commands
 * and events (`bindings.ts`); outside Tauri (browser preview, Playwright) the library is empty.
 */
export type {
  AlbumItem,
  AlbumPage,
  ArtistItem,
  ArtistPage,
  ImportFinished,
  ImportProgress,
  LibraryStatus,
  PageCursor,
  TrackItem,
} from "./bindings";

export const EMPTY_STATUS: LibraryStatus = {
  root: null,
  defaultRoot: "",
  tracks: 0,
  artists: 0,
  albums: 0,
  unknown: 0,
  durationSec: 0,
  bytes: 0,
  importing: false,
};

const unwrap = <T>(r: { status: "ok"; data: T } | { status: "error"; error: unknown }): T => {
  if (r.status === "error") throw r.error;
  return r.data;
};

export const getStatus = async (): Promise<LibraryStatus> =>
  isTauri() ? commands.libraryStatus() : EMPTY_STATUS;

export const pickFolder = async (): Promise<string | null> =>
  isTauri() ? commands.libraryPickFolder() : null;

export const setRoot = async (path: string): Promise<LibraryStatus> =>
  unwrap(await commands.librarySetRoot(path));

/** Imports files/folders; `"rootNeeded"` when no library folder has been chosen yet. */
export const importPaths = async (paths: string[]) =>
  isTauri() ? commands.importPaths(paths) : ("started" as const);

export const cancelImport = (): void => {
  if (isTauri()) void commands.importCancel();
};

export const albumsPage = async (after: PageCursor | null): Promise<AlbumPage> =>
  isTauri() ? unwrap(await commands.collectionAlbums(after, 200)) : { rows: [], next: null };

export const artistsPage = async (after: PageCursor | null): Promise<ArtistPage> =>
  isTauri() ? unwrap(await commands.collectionArtists(after, 200)) : { rows: [], next: null };

export const albumTracks = async (id: number): Promise<TrackItem[]> =>
  isTauri() ? unwrap(await commands.albumTracks(id)) : [];

export const artistTracks = async (id: number): Promise<TrackItem[]> =>
  isTauri() ? unwrap(await commands.artistTracks(id)) : [];

/** Plays `ids` as one context from `start` (FR-PLAY-006). */
export const playTracks = (ids: number[], start: number): void => {
  if (isTauri()) void commands.playTracks(ids, start);
};

type Off = () => void;
const listen = async <T>(
  ev: { listen: (cb: (e: { payload: T }) => void) => Promise<Off> },
  cb: (p: T) => void,
): Promise<Off> => (isTauri() ? ev.listen((e) => cb(e.payload)) : () => {});

export const onImportProgress = (cb: (p: ImportProgress) => void) =>
  listen(events.importProgress, cb);
export const onImportFinished = (cb: (p: ImportFinished) => void) =>
  listen(events.importFinished, cb);
export const onLibraryChanged = (cb: () => void) => listen(events.libraryChanged, () => cb());
export const onRootNeeded = (cb: () => void) => listen(events.libraryRootNeeded, () => cb());
