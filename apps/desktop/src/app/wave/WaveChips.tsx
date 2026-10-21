import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type WaveComposition, type WaveOff } from "@/shared/ipc/bindings";
import { useWeather } from "@/shared/ipc/context";
import { useEventOffer } from "@/shared/ipc/events";
import { Chip } from "@/shared/ui/Chip";

type Key = keyof WaveOff;

/**
 * «Состав волны» (T5.16, FR-LEARN-010, `14` §17; `01c` W-02 — the YouTube Music tuner): chips
 * under the player for what steers the wave now — season, time of day, weather, the event with
 * its countdown, taste. A press switches the influence off for this session (weight → 0, the
 * upcoming tracks are picked again); another press brings it back. A new wave starts with all on.
 */
export function WaveChips({ profile }: { profile: string }) {
  const { t } = useTranslation("shell");
  const weather = useWeather();
  const { tick } = useEventOffer();
  const [c, setC] = useState<WaveComposition | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: weather, event and wave changes are the triggers.
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    commands
      .waveComposition()
      .then((x) => alive && setC(x))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [weather, tick, profile]);
  if (!c) return null;
  const toggle = (k: Key) => {
    const off = { ...c.off, [k]: !c.off[k] };
    setC({ ...c, off }); // optimistic (DESIGN.md §1)
    void commands.waveSetOff(off);
  };
  const chips: [Key, string][] = [
    ["season", t(`wave.chips.season.${c.season}`)],
    ["context", t(`wave.chips.daypart.${c.daypart}`)],
  ];
  if (c.weather) chips.push(["context", t(`wave.chips.weather.${c.weather}`)]);
  if (c.event) {
    const name = t(`events.name.${c.event}`);
    chips.push([
      "event",
      c.eventDays > 0
        ? t("wave.chips.eventIn", { event: name, count: c.eventDays })
        : t("wave.chips.eventNow", { event: name }),
    ]);
  }
  chips.push(["taste", t("wave.chips.taste")]);
  return (
    <fieldset
      data-testid="wave-chips"
      aria-label={t("wave.chips.label")}
      className="relative m-0 flex flex-wrap justify-center gap-2 border-0 p-0"
    >
      {chips.map(([k, label]) => (
        <Chip key={label} active={!c.off[k]} onClick={() => toggle(k)}>
          {label}
        </Chip>
      ))}
    </fieldset>
  );
}
