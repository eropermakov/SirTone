import { isTauri } from "@tauri-apps/api/core";
import { commands, type PlaybackSnapshot } from "./bindings";
import type { LibraryStatus } from "./library";
import { EMPTY_SNAPSHOT } from "./playback";
import { getSettings, type SettingsDoc } from "./settings";

export type BootstrapState = {
  version: string;
  settings: SettingsDoc;
  playback: PlaybackSnapshot;
  library: LibraryStatus | null;
};

/**
 * Hydration in one call (T0.11, docs/03 §5): settings + player snapshot, so a (re)created window
 * draws live state in its first frame. Outside Tauri: web settings fallback, empty player.
 */
export async function bootstrap(): Promise<BootstrapState> {
  if (!isTauri()) {
    return {
      version: __APP_VERSION__,
      settings: await getSettings(),
      playback: EMPTY_SNAPSHOT,
      library: null,
    };
  }
  const b = await commands.appBootstrap();
  return {
    version: b.version,
    settings: b.settings as SettingsDoc,
    playback: b.playback ?? EMPTY_SNAPSHOT,
    library: b.library,
  };
}
