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

/** «Сбросить обучение» (docs/14 §8.4): the wave learns again from now; the journal stays. */
export const resetLearning = async (): Promise<void> => {
  if (isTauri()) unwrap(await commands.waveResetLearning());
};

/** Manual tags of a track (FR-LEARN-013). */
export const trackTags = async (track: number): Promise<string[]> =>
  isTauri() ? unwrap(await commands.trackTags(track)) : [];

export const setTag = async (track: number, tag: string, on: boolean): Promise<void> => {
  if (isTauri()) unwrap(await commands.trackTagSet(track, tag, on));
};
