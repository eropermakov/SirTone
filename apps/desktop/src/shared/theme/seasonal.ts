import { isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";
import { commands, type ThemeStateDto } from "@/shared/ipc/bindings";

/**
 * «Тема по сезону» (T4.18, FR-THEME-001…005, `14` §12, D-103): today's theme from Rust (seed,
 * visualiser palette, cover variant, effect). Rust decides by the local date, so the colour moves
 * once a day; the page asks again on start, every hour and when the window comes back (a laptop
 * may sleep past midnight). `null` — the switch is off (or the browser preview).
 */
export type SeasonTheme = ThemeStateDto;

let current: SeasonTheme | null = null;
let enabled = false;
let timer: number | undefined;
const subs = new Set<() => void>();

const emit = () => {
  for (const s of subs) s();
};

async function refresh(): Promise<void> {
  if (!enabled || !isTauri()) return;
  try {
    const next = await commands.themeState();
    if (!enabled) return;
    if (
      !current ||
      current.id !== next.id ||
      current.seed !== next.seed ||
      current.viz.join() !== next.viz.join()
    ) {
      current = next;
      emit();
    }
  } catch {
    // Keep what we have: the theme is decoration.
  }
}

const onVisible = () => {
  if (!document.hidden) void refresh();
};

/** Switch on/off; on → asks at once and keeps the theme current. */
export function setSeasonalEnabled(on: boolean): void {
  if (on === enabled) return;
  enabled = on;
  if (on) {
    void refresh();
    timer = window.setInterval(() => void refresh(), 60 * 60 * 1000);
    document.addEventListener("visibilitychange", onVisible);
  } else {
    window.clearInterval(timer);
    document.removeEventListener("visibilitychange", onVisible);
    current = null;
    emit();
  }
}

export function seasonTheme(): SeasonTheme | null {
  return current;
}

export function useSeasonTheme(): SeasonTheme | null {
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => current,
  );
}

let themeViz: string[] | null = null;
const vizSubs = new Set<() => void>();

/** The shell says which visualiser palette the theme brings (`null` — follow the accent). */
export function setThemeViz(v: string[] | null): void {
  if ((v ?? []).join() === (themeViz ?? []).join()) return;
  themeViz = v;
  for (const s of vizSubs) s();
}

export function useThemeViz(): string[] | null {
  return useSyncExternalStore(
    (cb) => {
      vizSubs.add(cb);
      return () => vizSubs.delete(cb);
    },
    () => themeViz,
  );
}

/** Test/diagnostic hook: replace the theme as if Rust answered (e2e). */
export function __setSeasonTheme(t: SeasonTheme | null): void {
  current = t;
  emit();
}

let lastInput = 0;
let watching = false;

/** Resolves once there has been no pointer, key or wheel input for `quietMs` (`14` §12:
 * the daily drift never starts during active interaction). */
export function whenIdle(quietMs = 2000, signal?: AbortSignal): Promise<void> {
  if (!watching) {
    watching = true;
    const mark = () => {
      lastInput = performance.now();
    };
    for (const e of ["pointerdown", "pointermove", "keydown", "wheel", "scroll"]) {
      window.addEventListener(e, mark, { capture: true, passive: true });
    }
  }
  return new Promise((resolve) => {
    const tick = () => {
      if (signal?.aborted) return;
      if (performance.now() - lastInput >= quietMs) resolve();
      else window.setTimeout(tick, 250);
    };
    tick();
  });
}
