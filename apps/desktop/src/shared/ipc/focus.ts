import { isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";
import { commands, events, type FocusStatus } from "./bindings";

/** Focus (T4.24, FR-FOCUS-001/002): commands and a shared store of the status from Rust. */
const OFF: FocusStatus = { phase: "off", remainingMs: 0, segmentMs: 0, paused: false, today: 0 };
let current: { s: FocusStatus; at: number } = { s: OFF, at: 0 };
const subs = new Set<() => void>();
let started = false;

function set(s: FocusStatus) {
  current = { s, at: performance.now() };
  for (const cb of subs) cb();
}

function ensure() {
  if (started || !isTauri()) return;
  started = true;
  void commands
    .focusStatus()
    .then(set)
    .catch(() => {});
  void events.focusStatus.listen((e) => set(e.payload));
}

export function useFocusStatus(): { s: FocusStatus; at: number } {
  ensure();
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => current,
  );
}

/** `work`/`brk` minutes — a preset; 0, 0 — the settings. */
export async function focusStart(work: number, brk: number): Promise<void> {
  if (isTauri()) await commands.focusStart(work, brk);
}

export async function focusStop(): Promise<void> {
  if (isTauri()) await commands.focusStop();
}

export async function focusPause(paused: boolean): Promise<void> {
  if (isTauri()) await commands.focusPause(paused);
}

export async function focusSkip(): Promise<void> {
  if (isTauri()) await commands.focusSkip();
}
