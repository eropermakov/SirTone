import { isTauri } from "@tauri-apps/api/core";
import { commands, type StorageInfo } from "./bindings";

export type { StorageInfo };

/** «Хранилище» (T2.14, FR-DATA-004): service data sizes; old logs cleanup. */
export const storageInfo = async (): Promise<StorageInfo | null> =>
  isTauri() ? commands.storageInfo() : null;

export const clearOldLogs = async (): Promise<number> => {
  if (!isTauri()) return 0;
  const r = await commands.storageClearLogs();
  if (r.status === "error") throw r.error;
  return r.data;
};
