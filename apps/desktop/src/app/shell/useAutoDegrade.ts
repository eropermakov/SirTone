import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { getSettings, onSettingsChanged, patchSettings } from "@/shared/ipc/settings";
import { FrameWatch } from "@/shared/theme/autodegrade";
import { applyPerformance, profile } from "@/shared/theme/performance";
import { notify } from "@/shared/ui/notify";

type Perf = {
  level?: number;
  auto_degrade?: boolean;
  degraded_from?: number | null;
  overrides?: Record<string, unknown>;
};

/**
 * Auto-degrade (T4.16, FR-PERF-004, D-47, `06` §6.2 — copy of Wallpaper Engine / Chrome Energy
 * Saver): watches the frame times; when the rule fires the level goes down by one, «sticks»
 * (`degraded_from` remembers the chosen one) and a toast offers «Вернуть». A screen change
 * (`screenKey`), a visualiser mode or theme change start the grace period anew.
 */
export function useAutoDegrade(screenKey: unknown): void {
  const { t } = useTranslation("shell");
  const watch = useRef(new FrameWatch());

  // biome-ignore lint/correctness/useExhaustiveDependencies: a screen change is the trigger itself.
  useEffect(() => {
    watch.current.reset(performance.now());
  }, [screenKey]);

  useEffect(() => {
    let alive = true;
    let raf = 0;
    let off: (() => void) | undefined;
    const w = watch.current;
    w.reset(performance.now());
    const reset = () => w.reset(performance.now());
    onSettingsChanged(reset).then((u) => {
      if (alive) off = u;
      else u();
    });
    document.addEventListener("visibilitychange", reset);
    const degrade = async () => {
      const doc = await getSettings();
      const p = (doc.performance ?? {}) as Perf;
      const level = p.level ?? 3;
      if (p.auto_degrade === false || level <= 0) return;
      const from = p.degraded_from ?? level;
      await patchSettings({ performance: { level: level - 1, degraded_from: from } });
      applyPerformance(profile(level - 1, p.overrides ?? {}));
      notify(t("perf.degraded"), {
        label: t("perf.restore"),
        onClick: () => {
          void patchSettings({ performance: { level: from, degraded_from: null } });
          applyPerformance(profile(from, p.overrides ?? {}));
        },
      });
    };
    const loop = (now: number) => {
      if (!alive) return;
      w.frame(now);
      if (w.check(now)) void degrade();
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    // Diagnostics for the smoke test.
    (window as unknown as { __sirtoneFrames?: FrameWatch }).__sirtoneFrames = w;
    return () => {
      alive = false;
      cancelAnimationFrame(raf);
      document.removeEventListener("visibilitychange", reset);
      off?.();
    };
  }, [t]);
}
