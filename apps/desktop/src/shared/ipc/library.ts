import { isTauri } from "@tauri-apps/api/core";
import {
  type AlbumPage,
  type ArtistPage,
  commands,
  events,
  type GenreItem,
  type ImportFinished,
  type ImportProgress,
  type LibraryStatus,
  type PageCursor,
  type PlayContext,
  type RemovalInfo,
  type TrackAssignment,
  type TrackDetails,
  type TrackItem,
  type TrackPage,
  type UnknownItem,
  type WhyReason,
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
  PlayContext,
  TrackAssignment,
  TrackItem,
  UnknownItem,
} from "./bindings";
export type { GenreItem, RemovalInfo, TrackDetails };

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
  offline: false,
  goneQuestion: null,
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

/** «Они удалены»: mark the vanished files missing (FR-FS-003). */
export const confirmMissing = (): void => {
  if (isTauri()) void commands.libraryConfirmMissing();
};

/** «Not now»: keep the library as it is. */
export const dismissQuestion = (): void => {
  if (isTauri()) void commands.libraryDismissQuestion();
};

/** Plays `ids` as one context from `start` (FR-PLAY-006); `context` goes to the journal. */
export const playTracks = (ids: number[], start: number, context: PlayContext): void => {
  if (isTauri()) void commands.playTracks(ids, start, context);
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

export const unknownTracks = async (): Promise<TrackItem[]> =>
  isTauri() ? unwrap(await commands.unknownTracks()) : [];

/** «Неизвестные» with reasons (docs/05 §4.7). */
export const unknownList = async (): Promise<UnknownItem[]> =>
  isTauri() ? unwrap(await commands.unknownList()) : [];

/** «Назначить»: who and what the track is; the file moves to its canonical place. */
export const assignTrack = async (track: number, a: TrackAssignment): Promise<void> => {
  if (isTauri()) unwrap(await commands.trackAssign(track, a));
};

/** Track card (FR-LIB-006): format, size, absolute path, tags. */
export const trackDetails = async (track: number): Promise<TrackDetails> => {
  if (!isTauri()) throw new Error("no library outside Tauri");
  return unwrap(await commands.trackDetails(track));
};

/** FR-DATA-001: `withFile` sends the file to the Recycle Bin; otherwise only the app forgets it. */
export const removeTrack = async (track: number, withFile: boolean): Promise<void> => {
  if (isTauri()) unwrap(await commands.trackRemove(track, withFile));
};

/** «Показать в проводнике» (Q-23). */
export const revealTrack = async (track: number): Promise<void> => {
  if (isTauri()) unwrap(await commands.trackReveal(track));
};

/** FR-DATA-002: how many tracks and bytes deleting an artist takes. */
export const artistRemovalInfo = async (artist: number): Promise<RemovalInfo> =>
  isTauri() ? unwrap(await commands.artistRemovalInfo(artist)) : { tracks: 0, bytes: 0 };

export const removeArtist = async (artist: number, withFile: boolean): Promise<void> => {
  if (isTauri()) unwrap(await commands.artistRemove(artist, withFile));
};

/** Genres from tags (FR-LIB-002; spellings merged). */
export const collectionGenres = async (): Promise<GenreItem[]> =>
  isTauri() ? unwrap(await commands.collectionGenres()) : [];

export const genreTracks = async (key: string): Promise<TrackItem[]> =>
  isTauri() ? unwrap(await commands.genreTracks(key)) : [];

/** Every playable track id (keyset pages of 500) — «Слушать всё вперемешку». */
export const allTrackIds = async (): Promise<number[]> => {
  if (!isTauri()) return [];
  const ids: number[] = [];
  let after: PageCursor | null = null;
  for (;;) {
    const page: TrackPage = unwrap(await commands.collectionTracks(after, 500));
    ids.push(...page.rows.map((r) => r.id));
    if (!page.next) return ids;
    after = page.next;
  }
};

/** «Моя волна» (Stage 3): the engine picks and plays; returns how many tracks were queued. */
export const waveStart = async (): Promise<number> =>
  isTauri() ? unwrap(await commands.waveStart()) : 0;

/** «Почему этот трек» for a track of the running wave (empty outside the wave). */
export const waveWhy = async (track: number): Promise<WhyReason[]> =>
  isTauri() ? commands.waveWhy(track) : [];

/** The slider moved: the upcoming wave tracks are picked again. */
export const waveRetune = (): void => {
  if (isTauri()) void commands.waveRetune();
};
