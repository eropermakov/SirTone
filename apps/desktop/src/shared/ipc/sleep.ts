import { isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";
import { commands, events, type SleepCaps, type SleepStatus } from "./bindings";

/**
 * Sleep timer (T4.21–T4.23, FR-SLEEP-001…007): the commands and a shared store of the status
 * Rust sends (phase, remaining time). The remaining time ticks locally between events.
 */
export type SleepKind = "minutes" | "track" | "tracks";

const OFF: SleepStatus = { phase: "off", kind: null, remainingMs: 0, tracksLeft: null };
let current: { s: SleepStatus; at: number } = { s: OFF, at: 0 };
const subs = new Set<() => void>();
let started = false;

function set(s: SleepStatus) {
  current = { s, at: performance.now() };
  for (const cb of subs) cb();
}

function ensure() {
  if (started || !isTauri()) return;
  started = true;
  void commands
    .sleepStatus()
    .then(set)
    .catch(() => {});
  void events.sleepStatus.listen((e) => set(e.payload));
}

/** The status and when it came (for a local countdown). */
export function useSleepStatus(): { s: SleepStatus; at: number } {
  ensure();
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => current,
  );
}

export function sleepStatusNow(): SleepStatus {
  return current.s;
}

/** Fading, final or woken: the person may be asleep. */
export const sleepFading = (s: SleepStatus) =>
  s.phase === "fading" || s.phase === "final" || s.phase === "woken";

export async function sleepStart(kind: SleepKind, value: number): Promise<void> {
  if (isTauri()) await commands.sleepStart(kind, value);
}

export async function sleepCancel(): Promise<void> {
  if (isTauri()) await commands.sleepCancel();
}

export async function sleepExtend(): Promise<void> {
  if (isTauri()) await commands.sleepExtend();
}

/** Any action in the window while the timer fades (FR-SLEEP-006). */
export async function sleepWake(): Promise<void> {
  if (isTauri()) await commands.sleepWake();
}

export async function sleepCaps(): Promise<SleepCaps> {
  return isTauri() ? commands.sleepCaps() : { hibernate: false, modernStandby: false };
}
