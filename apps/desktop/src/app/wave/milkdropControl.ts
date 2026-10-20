import { useSyncExternalStore } from "react";

/**
 * The MilkDrop controls on the wave screen (T4.3, Kodi «Lock / Rate», Poweramp «замок»,
 * `01a` §B.2): the preset on screen, its rating, the lock. The background owns the engine and
 * binds the actions; the toolbar reads and calls them.
 */
type State = { preset: string; rating: number; locked: boolean };
type Actions = { rate: (rating: number) => void; skip: () => void };

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
  toggleLock(): void {
    milkdropControl.set({ locked: !state.locked });
  },
  subscribe(cb: () => void): () => void {
    subs.add(cb);
    return () => subs.delete(cb);
  },
};

export function useMilkdropControl(): State {
  return useSyncExternalStore(milkdropControl.subscribe, milkdropControl.get);
}
