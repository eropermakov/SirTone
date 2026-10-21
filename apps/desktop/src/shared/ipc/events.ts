import { isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";
import { refreshSeasonTheme } from "@/shared/theme/seasonal";
import { commands, type EventInfo, type EventOffer, events } from "./bindings";

export type { EventInfo, EventOffer };

/**
 * Event waves (T5.13–T5.14, FR-EVT-001…004): the card Rust offers (≤ 1 a day), the answers and
 * the per-event list for the settings. Every change of the offer — or of the accepted event —
 * also re-reads the theme (the event's theme until its window ends, then the season again).
 */
const NONE: EventOffer = { id: null, daysToPeak: 0, strong: false, atmospheric: false };
let state = { offer: NONE, tick: 0 };
const subs = new Set<() => void>();
let started = false;

function set(o: EventOffer) {
  state = { offer: o, tick: state.tick + 1 };
  for (const s of subs) s();
  void refreshSeasonTheme();
}

function ensure() {
  if (started || !isTauri()) return;
  started = true;
  void commands
    .eventOffer()
    .then(set)
    .catch(() => {});
  void events.eventOffer.listen((e) => set(e.payload));
}

/** The card now (`id: null` — none) and a counter that moves on every event change. */
export function useEventOffer(): { offer: EventOffer; tick: number } {
  ensure();
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => state,
  );
}

export type EventAnswer = "accept" | "later" | "year" | "never";

export async function eventAnswer(id: string, answer: EventAnswer): Promise<void> {
  // Optimistic: the card goes at once (DESIGN.md §1); Rust confirms with the event.
  set(NONE);
  const r = await commands.eventAnswer(id, answer);
  if (r.status === "error") throw r.error;
}

export const eventList = async (): Promise<EventInfo[]> => (isTauri() ? commands.eventList() : []);

export const eventRefresh = async (): Promise<void> => {
  if (isTauri()) await commands.eventRefresh();
};
