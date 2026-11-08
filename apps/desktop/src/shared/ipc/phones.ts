import { Channel } from "@tauri-apps/api/core";
import {
  type AppError,
  commands,
  type ErrorCode,
  type InstallProgress,
  type PhoneAccount,
  type PhoneInfo,
  type PhoneSelection,
  type PhonesStatus,
  type PickItem,
  type PickLists,
  type PullDone,
  type SelectionSize,
  type SendParts,
  type TransferDone,
  type TransferProgress,
  type TwoFactorReply,
  type TwoFactorRequest,
} from "./bindings";

/** Hub «Телефоны» (stage 8, FR-HUB-001…004): phones this PC sees and the iPhone wizard. */
export type {
  PhoneAccount,
  PhoneInfo,
  PhoneSelection,
  PhonesStatus,
  PickItem,
  PickLists,
  TransferDone,
  TwoFactorReply,
  TwoFactorRequest,
};

const unwrap = <T>(r: { status: "ok"; data: T } | { status: "error"; error: AppError }): T => {
  if (r.status === "error") throw r.error;
  return r.data;
};

/** The code of a failed hub call (the screen words it); unknown failures read as `Internal`. */
export const errorCode = (e: unknown): ErrorCode =>
  typeof e === "object" && e !== null && "code" in e ? (e as AppError).code : "Internal";

export function phonesList(): Promise<PhonesStatus> {
  return commands.phonesList();
}

export function phoneAccount(udid: string): Promise<PhoneAccount> {
  return commands.phoneAccount(udid);
}

/** Сиртон is already on the phone, wherever it was installed from (FR-HUB-003). */
export function phoneHasApp(udid: string): Promise<boolean> {
  return commands.phoneHasApp(udid);
}

/**
 * Wizard step 6: logs in with the Apple ID; `onAsk` gets each 2FA request and the form answers
 * with {@link answerTwoFactor}. The password goes to the core once and is never kept here.
 */
export async function appleLogin(
  udid: string,
  appleId: string,
  password: string,
  onAsk: (r: TwoFactorRequest) => void,
): Promise<void> {
  const asks = new Channel<TwoFactorRequest>(onAsk);
  unwrap(await commands.phoneAppleLogin(udid, appleId, password, asks));
}

export function answerTwoFactor(reply: TwoFactorReply): Promise<void> {
  return commands.phoneApple2fa(reply);
}

/** Wizard step 7: signs and installs over the cable; `onProgress` gets 0…1. */
export async function installOnPhone(udid: string, onProgress: (f: number) => void): Promise<void> {
  const progress = new Channel<InstallProgress>((p) => onProgress(p.fraction ?? 0));
  unwrap(await commands.phoneInstall(udid, progress));
}

/**
 * «Перенести всё» (M6.2): the PC library into the app's inbox on the phone, as originals or
 * economy copies made by the PC (D-146) (the app applies it
 * when opened); `onProgress` gets 0…1. A repeat after a cut cable resumes.
 */
export type Quality = "original" | "economy" | "max";

/**
 * Only what the phone lacks goes (D-152); `selection` — «Выбранная» музыка (`null` — all of it).
 * `already` in the answer: the phone had everything, nothing was sent.
 */
export async function sendToPhone(
  udid: string,
  quality: Quality,
  parts: SendParts,
  onProgress: (f: number) => void,
  selection: PhoneSelection | null = null,
): Promise<TransferDone> {
  const progress = new Channel<TransferProgress>((p) => onProgress(p.fraction ?? 0));
  return unwrap(await commands.phoneSend(udid, quality, parts, selection, progress));
}

/** The «Выбранная» picker's lists: artists, albums, genres, playlists (D-152). */
export async function pickLists(): Promise<PickLists> {
  return unwrap(await commands.phonePickLists());
}

/** The chosen music's size before the phone is asked what it has. */
export async function selectionSize(selection: PhoneSelection): Promise<SelectionSize> {
  return unwrap(await commands.phoneSelectionSize(selection));
}

/** An empty choice. */
export const NO_SELECTION: PhoneSelection = {
  artists: [],
  albums: [],
  genres: [],
  playlists: [],
  tracks: [],
};

export const selectionCount = (s: PhoneSelection): number =>
  s.artists.length + s.albums.length + s.genres.length + s.playlists.length + s.tracks.length;

export async function forgetPhone(udid: string): Promise<void> {
  unwrap(await commands.phoneForget(udid));
}

/**
 * «Забрать с телефона» (MR §6.2): the phone's library comes to this PC — only the music it lacks,
 * plus plays, marks and playlists (D-140); works with the app closed. `onProgress` gets 0…1.
 */
export async function pullFromPhone(
  udid: string,
  onProgress: (f: number) => void,
): Promise<PullDone> {
  const progress = new Channel<TransferProgress>((p) => onProgress(p.fraction ?? 0));
  return unwrap(await commands.phonePull(udid, progress));
}
