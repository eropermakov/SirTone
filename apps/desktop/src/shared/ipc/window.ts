import { isTauri } from "@tauri-apps/api/core";
import { commands, type HotkeyStatus } from "./bindings";

export type { HotkeyAction, HotkeyStatus } from "./bindings";

/** «Запускать вместе с Windows» is offered only in the installed copy (FR-SHELL-007). */
export async function autostartAvailable(): Promise<boolean> {
  return isTauri() ? Boolean(await commands.autostartAvailable()) : false;
}

/** Turns start-with-Windows on/off; resolves to the real state. */
export async function setAutostart(on: boolean): Promise<boolean> {
  if (!isTauri()) return false;
  const r = await commands.autostartSet(on);
  if (r.status === "error") throw r.error;
  return r.data;
}

/** Tints the native Windows caption to the resolved app theme (S-20 (a), D-60). */
export function setWindowTheme(theme: "dark" | "light"): void {
  if (isTauri()) void commands.windowSetTheme(theme === "dark");
}

/** Configured global hotkeys and which ones Windows refused (FR-WIN-001). */
export async function hotkeysStatus(): Promise<HotkeyStatus[]> {
  if (!isTauri()) return [];
  const list = await commands.hotkeysStatus();
  return Array.isArray(list) ? list : [];
}
