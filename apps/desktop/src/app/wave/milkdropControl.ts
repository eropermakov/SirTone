import { useSyncExternalStore } from "react";
import { patchSettings } from "@/shared/ipc/settings";

/**
 * The MilkDrop controls on the wave screen (T4.3, Kodi «Lock / Rate», Poweramp «замок»,
 * `01a` §B.2): the preset on screen, its rating, the lock. The background owns the engine and
 * binds the actions; the toolbar and «Мои пресеты» read and call them.
 *
 * The lock is a setting (`viz.milkdrop_lock` — the preset's name, owner 2026-10-20, D-105): the
 * locked preset stays across track changes, mode switches and restarts.
 */
type State = { preset: string; rating: number; locked: boolean };
type Actions = {
  rate: (rating: number) => void;
  skip: () => void;
  /** Rate any preset by name (the «Мои пресеты» list). */
  rateName: (name: string, rating: number) => void;
  /** Show this preset now. */
  show: (name: string) => void;
  /** Every preset and its rating (♥ 1, ✕ −1). */
  list: () => { name: string; rating: number }[];
};

let state: State = { preset: "", rating: 0, locked: false };
let actions: Actions | null = null;
const subs = new Set<() => void>();

export const milkdropControl = {
  get: (): State => state,
  set(patch: Partial<State>): void {
    state = { ...state, ...patch };
    for (const s of subs) s();
  },
  bind(a: Actions | null): void {
    actions = a;
    if (!a) milkdropControl.set({ preset: "", rating: 0 });
  },
  rate(rating: number): void {
    actions?.rate(rating);
  },
  skip(): void {
    actions?.skip();
  },
  rateName(name: string, rating: number): void {
    actions?.rateName(name, rating);
  },
  show(name: string): void {
    actions?.show(name);
  },
  list(): { name: string; rating: number }[] {
    return actions?.list() ?? [];
  },
  /** 🔒: keep the preset on screen (saved), or let it go. */
  toggleLock(): void {
    const lock = state.locked ? null : state.preset || null;
    milkdropControl.set({ locked: !!lock });
    void patchSettings({ viz: { milkdrop_lock: lock } }).catch(() =>
      milkdropControl.set({ locked: !lock }),
    );
  },
  subscribe(cb: () => void): () => void {
    subs.add(cb);
    return () => subs.delete(cb);
  },
};

export function useMilkdropControl(): State {
  return useSyncExternalStore(milkdropControl.subscribe, milkdropControl.get);
}
