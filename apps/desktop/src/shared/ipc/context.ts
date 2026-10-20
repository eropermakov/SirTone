import { isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";
import { type City, commands, events, type WeatherNow } from "./bindings";

export type { City, WeatherNow };

/** The context (T5.5, FR-CTX-001/002): the city search, «определить автоматически», weather. */
const unwrap = <T>(r: { status: "ok"; data: T } | { status: "error"; error: unknown }): T => {
  if (r.status === "error") throw r.error;
  return r.data;
};

export const citySearch = async (q: string): Promise<City[]> =>
  isTauri() ? unwrap(await commands.citySearch(q)) : [];

export const cityLocate = async (): Promise<City> => unwrap(await commands.cityLocate());

const NONE: WeatherNow = {
  class: null,
  tempC: null,
  isDay: true,
  drift: 0,
  intensity: 0,
  fetchedAt: null,
  fresh: false,
};
let current = NONE;
const subs = new Set<() => void>();
let started = false;

function set(w: WeatherNow) {
  current = w;
  for (const s of subs) s();
}

function ensure() {
  if (started || !isTauri()) return;
  started = true;
  void commands
    .weatherGet()
    .then(set)
    .catch(() => {});
  void events.weatherNow.listen((e) => set(e.payload));
}

/** The weather now (Rust sends it on every fetch); `class: null` — none or older than 3 h. */
export function useWeather(): WeatherNow {
  ensure();
  return useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => current,
  );
}
