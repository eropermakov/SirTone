import { Channel } from "@tauri-apps/api/core";
import {
  type AppError,
  commands,
  type ErrorCode,
  type InstallProgress,
  type PhoneAccount,
  type PhoneInfo,
  type PhonesStatus,
  type TransferDone,
  type TransferProgress,
  type TwoFactorReply,
  type TwoFactorRequest,
} from "./bindings";

/** Hub «Телефоны» (stage 8, FR-HUB-001…004): phones this PC sees and the iPhone wizard. */
export type { PhoneAccount, PhoneInfo, PhonesStatus, TwoFactorReply, TwoFactorRequest };

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

export async function sendToPhone(
  udid: string,
  quality: Quality,
  onProgress: (f: number) => void,
): Promise<TransferDone> {
  const progress = new Channel<TransferProgress>((p) => onProgress(p.fraction ?? 0));
  return unwrap(await commands.phoneSend(udid, quality, progress));
}

export async function forgetPhone(udid: string): Promise<void> {
  unwrap(await commands.phoneForget(udid));
}
