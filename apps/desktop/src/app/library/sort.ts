import type { AlbumItem, ArtistItem } from "@/shared/ipc/library";

/**
 * Collection sort (FR-LIB-008; `01b` F-34): Spotify's «Недавно добавленные / По алфавиту / По
 * автору», Apple's year; the choice is remembered per section (Spotify), choosing it again
 * reverses the order (Apple). Remembered in this browser profile only — a view convenience.
 */
export const ALBUM_SORTS = ["title", "artist", "year", "added"] as const;
export const ARTIST_SORTS = ["name", "tracks", "added"] as const;
export type AlbumSort = (typeof ALBUM_SORTS)[number];
export type ArtistSort = (typeof ARTIST_SORTS)[number];
export type Sort<K> = { by: K; reverse: boolean };

const KEY = (kind: string) => `sirtone.sort.${kind}`;

export function loadSort<K extends string>(kind: string, allowed: readonly K[], def: K): Sort<K> {
  try {
    const v = JSON.parse(localStorage.getItem(KEY(kind)) ?? "null") as Sort<K> | null;
    if (v && allowed.includes(v.by)) return { by: v.by, reverse: Boolean(v.reverse) };
  } catch {
    // no storage (private window, preview) — the default is fine
  }
  return { by: def, reverse: false };
}

export function saveSort(kind: string, s: Sort<string>) {
  try {
    localStorage.setItem(KEY(kind), JSON.stringify(s));
  } catch {
    // ignore
  }
}

/** Same option again → reverse (Apple Music). */
export const next = <K>(cur: Sort<K>, by: K): Sort<K> =>
  cur.by === by ? { by, reverse: !cur.reverse } : { by, reverse: false };

const coll = new Intl.Collator("ru", { sensitivity: "base", numeric: true });

export function sortAlbums(list: AlbumItem[], s: Sort<AlbumSort>): AlbumItem[] {
  const out = [...list];
  const cmp: Record<AlbumSort, (a: AlbumItem, b: AlbumItem) => number> = {
    title: (a, b) => coll.compare(a.title, b.title),
    artist: (a, b) => coll.compare(a.artist, b.artist) || (a.year ?? 0) - (b.year ?? 0),
    // Newest first, like Spotify; unknown years last.
    year: (a, b) => (b.year ?? -1) - (a.year ?? -1) || coll.compare(a.title, b.title),
    added: (a, b) => (b.addedAt ?? 0) - (a.addedAt ?? 0),
  };
  out.sort(cmp[s.by]);
  return s.reverse ? out.reverse() : out;
}

export function sortArtists(list: ArtistItem[], s: Sort<ArtistSort>): ArtistItem[] {
  const out = [...list];
  const cmp: Record<ArtistSort, (a: ArtistItem, b: ArtistItem) => number> = {
    name: (a, b) => coll.compare(a.name, b.name),
    tracks: (a, b) => b.tracks - a.tracks || coll.compare(a.name, b.name),
    added: (a, b) => (b.addedAt ?? 0) - (a.addedAt ?? 0),
  };
  out.sort(cmp[s.by]);
  return s.reverse ? out.reverse() : out;
}
