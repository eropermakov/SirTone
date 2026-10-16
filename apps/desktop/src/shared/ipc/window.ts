import { isTauri } from "@tauri-apps/api/core";
import { commands } from "./bindings";

/** Tints the native Windows caption to the resolved app theme (S-20 (a), D-60). */
export function setWindowTheme(theme: "dark" | "light"): void {
  if (isTauri()) void commands.windowSetTheme(theme === "dark");
}
