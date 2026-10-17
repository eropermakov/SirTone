import { isTauri } from "@tauri-apps/api/core";
import { commands, events, type UpdateReady, type UpdateStatus } from "./bindings";

/** Updates IPC (FR-UPD-001/002, D-76). Outside Tauri there is nothing to update. */
export type { UpdateReady, UpdateStatus };

export async function updateStatus(): Promise<UpdateStatus | null> {
  return isTauri() ? commands.updateStatus() : null;
}

/** «Проверить обновления»: the version found (already downloaded) or null. */
export async function checkForUpdate(): Promise<string | null> {
  if (!isTauri()) return null;
  const r = await commands.updateCheck();
  if (r.status === "error") throw r.error;
  return r.data;
}

/** «Обновить сейчас»: the installer closes Сиртон and starts the new version. */
export function installUpdate(): void {
  if (isTauri()) void commands.updateInstall();
}

export async function onUpdateReady(cb: (u: UpdateReady) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  return events.updateReady.listen((e) => cb(e.payload));
}
