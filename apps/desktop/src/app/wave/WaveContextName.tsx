import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { type WaveContextName as Ctx, commands } from "@/shared/ipc/bindings";
import { useWeather } from "@/shared/ipc/context";

/** Grammatical gender of the part of the day's noun (рассвет м., утро ср., ночь ж., сумерки мн.). */
const GENDER: Record<string, "m" | "n" | "f" | "pl"> = {
  dawn: "m",
  morning: "n",
  day: "m",
  evening: "m",
  dusk: "pl",
  night: "f",
};

/**
 * The contextual name of «Моя волна» (T5.11, FR-WAVE-007): «Снежный вечер» / «Осеннее утро» and
 * when it changes («обновится в 18:40»). Rust says the part of the day, the weather (fresh only)
 * and the season; the words and their agreement are here, in i18n.
 */
export function WaveContextName() {
  const { t, i18n } = useTranslation("shell");
  const weather = useWeather();
  const [ctx, setCtx] = useState<Ctx | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: a weather update is the trigger itself.
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    let timer: number | undefined;
    const load = () => {
      commands
        .waveContextName()
        .then((c) => {
          if (!alive) return;
          setCtx(c);
          // Again right after the part of the day changes (and at least hourly).
          const wait = Math.min(Math.max((c.nextAt ?? 0) - Date.now() + 2000, 5000), 3_600_000);
          timer = window.setTimeout(load, wait);
        })
        .catch(() => {});
    };
    load();
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [weather]);
  if (!ctx) return null;
  const g = GENDER[ctx.daypart] ?? "m";
  const adj = t(`wave.ctx.adj.${ctx.weather ?? ctx.season}.${g}`);
  const noun = t(`wave.ctx.noun.${ctx.daypart}`);
  const time = new Date(ctx.nextAt ?? Date.now()).toLocaleTimeString(i18n.language, {
    hour: "2-digit",
    minute: "2-digit",
  });
  return (
    <div data-testid="wave-context" className="relative flex flex-col items-center gap-1">
      <span className="t-card-title text-text-primary">{t("wave.ctx.name", { adj, noun })}</span>
      <span className="t-caption text-text-secondary">{t("wave.ctx.next", { time })}</span>
    </div>
  );
}
