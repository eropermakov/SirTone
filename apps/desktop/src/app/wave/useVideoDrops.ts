import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { onVideosAdded, videoClips, videoRemove, videoUrl } from "@/shared/ipc/viz";
import { notify } from "@/shared/ui/notify";
import { videoControl } from "./videoControl";

/** Longest clip accepted (`06` §7.4). */
const MAX_S = 60;

/** Plays at all, and how long: a hidden muted `<video>` reads the metadata. */
function probe(id: string): Promise<number | null> {
  return new Promise((resolve) => {
    const v = document.createElement("video");
    v.muted = true;
    v.preload = "metadata";
    const done = (d: number | null) => {
      v.removeAttribute("src");
      v.load();
      resolve(d);
    };
    const timer = window.setTimeout(() => done(null), 10_000);
    v.onloadedmetadata = () => {
      window.clearTimeout(timer);
      done(Number.isFinite(v.duration) ? v.duration : null);
    };
    v.onerror = () => {
      window.clearTimeout(timer);
      done(null);
    };
    v.src = videoUrl(id);
  });
}

/**
 * Clips dropped on the window (T4.4, FR-VIZ-005): Rust copies them in; here each one must play
 * and last ≤ 60 s, or it goes again; then a toast says what happened. Mounted once for the app.
 */
export function useVideoDrops(): void {
  const { t } = useTranslation("shell");
  useEffect(() => {
    let alive = true;
    let off: (() => void) | undefined;
    const reload = () => videoClips().then((clips) => alive && videoControl.set({ clips }));
    void reload();
    // A library switch or a test that placed clips asks for the list again.
    window.addEventListener("sirtone-videos-refresh", reload);
    onVideosAdded(async (e) => {
      let ok = 0;
      let refused = e.refused;
      for (const id of e.added) {
        const d = await probe(id);
        if (d !== null && d > 0 && d <= MAX_S) ok++;
        else {
          refused++;
          await videoRemove(id).catch(() => {});
        }
      }
      if (!alive) return;
      videoControl.set({ clips: await videoClips() });
      if (ok > 0) notify(t("viz.videoAdded", { count: ok }));
      if (refused > 0) notify(t("viz.videoRefused", { count: refused }));
    }).then((u) => {
      if (alive) off = u;
      else u();
    });
    return () => {
      alive = false;
      window.removeEventListener("sirtone-videos-refresh", reload);
      off?.();
    };
  }, [t]);
}
