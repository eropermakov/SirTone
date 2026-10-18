import { isTauri } from "@tauri-apps/api/core";
import { commands, type NeverEntry, type NeverScope, type TrackFeedback } from "./bindings";

export type { NeverEntry, NeverScope, TrackFeedback };

/** Player feedback (T2.19, FR-LEARN-003). Outside Tauri — nothing is stored. */
const unwrap = <T>(r: { status: "ok"; data: T } | { status: "error"; error: unknown }): T => {
  if (r.status === "error") throw r.error;
  return r.data;
};

const NONE: TrackFeedback = { liked: false, neverTrack: false, neverArtist: false };

export const trackFeedback = async (track: number): Promise<TrackFeedback> =>
  isTauri() ? unwrap(await commands.trackFeedback(track)) : NONE;

export const like = async (track: number, on: boolean): Promise<void> => {
  if (isTauri()) unwrap(await commands.feedbackLike(track, on));
};

/** «Не сейчас»: a minus in this context, then the next track. */
export const notNow = async (track: number): Promise<void> => {
  if (isTauri()) unwrap(await commands.feedbackNotNow(track));
};

export const never = async (scope: NeverScope, id: number, on: boolean): Promise<void> => {
  if (isTauri()) unwrap(await commands.feedbackNever(scope, id, on));
};

export const primaryArtist = async (track: number): Promise<number> => {
  if (!isTauri()) throw new Error("no library outside Tauri");
  return unwrap(await commands.trackPrimaryArtist(track));
};

export const neverList = async (): Promise<NeverEntry[]> =>
  isTauri() ? unwrap(await commands.neverList()) : [];
