import { useEffect } from "react";
import { getSettings, onSettingsChanged, type SettingsDoc } from "@/shared/ipc/settings";
import { applyPerformance, profile } from "@/shared/theme/performance";

type Perf = { level?: number; overrides?: Record<string, unknown> };

/** Applies the performance preset now and whenever settings change (T2.3). */
export function usePerformance(): void {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let alive = true;
    const apply = (doc: SettingsDoc) => {
      const p = (doc.performance ?? {}) as Perf;
      applyPerformance(profile(p.level ?? 3, p.overrides ?? {}));
    };
    getSettings().then(apply);
    onSettingsChanged(() => getSettings().then(apply)).then((u) => {
      if (alive) unlisten = u;
      else u();
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
}
