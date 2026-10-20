import { isTauri } from "@tauri-apps/api/core";
import { commands } from "./bindings";

/**
 * UI sounds (T4.15, FR-PLAY-016, `06` §8): played by the engine, not WebAudio (no extra audio
 * sessions); the engine drops one that comes < 80 ms after another and the volume/switch live in
 * the settings (`sfx`).
 */
export type Sfx =
  | "click"
  | "toggle_on"
  | "toggle_off"
  | "queue_add"
  | "drop"
  | "import_done"
  | "import_error"
  | "mode"
  | "focus_work_end"
  | "focus_break_end";

export function sfx(name: Sfx): void {
  if (isTauri()) void commands.uiSound(name);
}
