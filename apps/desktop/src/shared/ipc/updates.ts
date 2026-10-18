import { isTauri } from "@tauri-apps/api/core";
import { commands, type UpdateStatus } from "./bindings";

/**
 * Updates IPC (D-76): everything happens by itself in Rust; the UI only learns, once, that the
 * app was just updated. Outside Tauri there is nothing to update.
 */
export type { UpdateStatus };

export async function updateStatus(): Promise<UpdateStatus | null> {
  return isTauri() ? commands.updateStatus() : null;
}
