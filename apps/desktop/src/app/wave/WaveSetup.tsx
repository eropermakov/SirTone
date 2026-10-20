import {
  Ban,
  Briefcase,
  Car,
  Cloud,
  CloudDrizzle,
  CloudFog,
  CloudLightning,
  CloudRain,
  Coffee,
  Dumbbell,
  Feather,
  Flame,
  Flower2,
  Globe,
  Heart,
  History,
  Languages,
  Leaf,
  type LucideIcon,
  Moon,
  MoonStar,
  Music2,
  PartyPopper,
  Repeat,
  Smile,
  Snowflake,
  Sparkles,
  Sun,
  Sunrise,
  Sunset,
  Wand2,
  Zap,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { WaveQueryIpc } from "@/shared/ipc/bindings";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";

/**
 * «Настроить волну» (FR-WAVE-003, `14` §6, owner 2026-10-20 — like the old Яндекс Музыка
 * selector, `01c` W-02): the wave put together like a constructor — tiles in blocks, a press
 * adds, another press takes away; blocks combine. Saved (`wave.setup`), so «Моя волна» keeps
 * playing this way until «Сбросить». Temporary look until the owner's design (D-106).
 */
type Multi = "activity" | "character" | "mood" | "language" | "frequency";

const MULTI: { block: Multi; tiles: [string, LucideIcon][] }[] = [
  {
    block: "activity",
    tiles: [
      ["workout", Dumbbell],
      ["work", Briefcase],
      ["road", Car],
      ["rest", Coffee],
      ["party", PartyPopper],
      ["sleep", Moon],
    ],
  },
  {
    block: "character",
    tiles: [
      ["energetic", Zap],
      ["aggressive", Flame],
      ["calm", Feather],
    ],
  },
  {
    block: "mood",
    tiles: [
      ["happy", Smile],
      ["upbeat", Sun],
      ["calm", Leaf],
      ["sad", CloudRain],
      ["positive", Heart],
      ["negative", CloudFog],
    ],
  },
  {
    block: "language",
    tiles: [
      ["ru", Languages],
      ["foreign", Globe],
      ["none", Music2],
    ],
  },
  {
    block: "frequency",
    tiles: [
      ["loved", Heart],
      ["often", Repeat],
      ["forgotten", History],
    ],
  },
];

const WORKOUT_TEMPOS = ["light", "medium", "intense"] as const;
const WORKOUT = "workout";
const SLEEP = "sleep";
const WORK = "work";

const DAYPARTS: LucideIcon[] = [Sunrise, Sun, Sun, Sunset, Moon, MoonStar];
const WEATHER: LucideIcon[] = [Sun, Cloud, CloudFog, CloudDrizzle, Snowflake, CloudLightning];
const SEASONS: [string, LucideIcon][] = [
  ["auto", Wand2],
  ["winter", Snowflake],
  ["spring", Flower2],
  ["summer", Sun],
  ["autumn", Leaf],
  ["off", Ban],
];

/** How many blocks narrow the wave (the button's badge). */
export function setupCount(q: WaveQueryIpc | null): number {
  if (!q) return 0;
  const multi = MULTI.filter((m) => (q[m.block]?.length ?? 0) > 0).length;
  return (
    multi +
    (q.daypart != null ? 1 : 0) +
    (q.weather != null ? 1 : 0) +
    (q.season && q.season !== "auto" ? 1 : 0)
  );
}

function Tile({
  icon: Icon,
  label,
  on,
  onClick,
}: {
  icon: LucideIcon;
  label: string;
  on: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={onClick}
      className={cn(
        "flex w-24 flex-col items-center gap-2 rounded-(--radius-lg) border px-2 py-3 transition-colors duration-(--dur-fast)",
        on
          ? "border-accent-text bg-bg-selected text-text-primary"
          : "border-border text-text-secondary hover:bg-bg-hover",
      )}
    >
      <Icon size={24} strokeWidth={1.5} className={on ? "text-accent-text" : "text-icon"} />
      <span className="t-caption text-center leading-tight">{label}</span>
    </button>
  );
}

export function WaveSetup({
  initial,
  onClose,
  onPlay,
  onSleepWave,
  onFocus,
}: {
  initial: WaveQueryIpc | null;
  onClose: () => void;
  /** «Слушать» with this setup (saved by the caller). */
  onPlay: (q: WaveQueryIpc) => void;
  onSleepWave: () => void;
  onFocus: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [q, setQ] = useState<WaveQueryIpc>(initial ?? {});
  const toggle = (block: Multi, key: string) =>
    setQ((cur) => {
      const list = cur[block] ?? [];
      return {
        ...cur,
        [block]: list.includes(key) ? list.filter((k) => k !== key) : [...list, key],
      };
    });
  const activity = q.activity ?? [];
  const workout = activity.includes(WORKOUT);
  const sleeping = activity.includes(SLEEP);
  const hint = sleeping || activity.includes(WORK);
  return (
    <Dialog title={t("wave.setup.title")} closeLabel={tc("errors.close")} onClose={onClose} wide>
      <div className="flex max-h-[65vh] flex-col gap-6 overflow-auto pr-1">
        {MULTI.map(({ block, tiles }) => (
          <section key={block} className="flex flex-col gap-3">
            <h3 className="t-card-title text-text-primary">{t(`wave.setup.block.${block}`)}</h3>
            <div className="flex flex-wrap gap-2">
              {tiles.map(([key, icon]) => (
                <Tile
                  key={key}
                  icon={icon}
                  label={t(`wave.setup.${block}.${key}`)}
                  on={(q[block] ?? []).includes(key)}
                  onClick={() => toggle(block, key)}
                />
              ))}
            </div>
            {block === "activity" && workout && (
              <div className="flex flex-wrap gap-2">
                {WORKOUT_TEMPOS.map((k) => (
                  <Tile
                    key={k}
                    icon={Zap}
                    label={t(`wave.setup.workoutTempo.${k}`)}
                    on={q.workoutTempo === k}
                    onClick={() =>
                      setQ((c) => ({ ...c, workoutTempo: c.workoutTempo === k ? null : k }))
                    }
                  />
                ))}
              </div>
            )}
            {block === "activity" && hint && (
              <div className="flex flex-wrap items-center gap-3 rounded-(--radius-lg) bg-bg-layer px-4 py-3">
                <Sparkles size={16} strokeWidth={1.5} className="text-icon" />
                <span className="t-caption flex-1 text-text-secondary">
                  {sleeping ? t("wave.setup.hintSleep") : t("wave.setup.hintFocus")}
                </span>
                <Button
                  variant="secondary"
                  onClick={() => {
                    if (sleeping) onSleepWave();
                    else onFocus();
                    onClose();
                  }}
                >
                  {sleeping ? t("wave.setup.sleepWave") : t("wave.setup.focus")}
                </Button>
              </div>
            )}
          </section>
        ))}
        <section className="flex flex-col gap-3">
          <h3 className="t-card-title text-text-primary">{t("wave.setup.block.daypart")}</h3>
          <div className="flex flex-wrap gap-2">
            {DAYPARTS.map((icon, i) => (
              <Tile
                // biome-ignore lint/suspicious/noArrayIndexKey: dayparts are fixed codes 0…5
                key={i}
                icon={icon}
                label={t(`wave.setup.daypart.${i}`)}
                on={q.daypart === i}
                onClick={() => setQ((c) => ({ ...c, daypart: c.daypart === i ? null : i }))}
              />
            ))}
          </div>
        </section>
        <section className="flex flex-col gap-3">
          <h3 className="t-card-title text-text-primary">{t("wave.setup.block.weather")}</h3>
          <div className="flex flex-wrap gap-2">
            {WEATHER.map((icon, i) => (
              <Tile
                // biome-ignore lint/suspicious/noArrayIndexKey: weather classes are fixed codes 0…5
                key={i}
                icon={icon}
                label={t(`wave.setup.weather.${i}`)}
                on={q.weather === i}
                onClick={() => setQ((c) => ({ ...c, weather: c.weather === i ? null : i }))}
              />
            ))}
          </div>
        </section>
        <section className="flex flex-col gap-3">
          <h3 className="t-card-title text-text-primary">{t("wave.setup.block.season")}</h3>
          <div className="flex flex-wrap gap-2">
            {SEASONS.map(([key, icon]) => (
              <Tile
                key={key}
                icon={icon}
                label={t(`wave.setup.season.${key}`)}
                on={(q.season ?? "auto") === key}
                onClick={() => setQ((c) => ({ ...c, season: key }))}
              />
            ))}
          </div>
        </section>
      </div>
      <div className="flex flex-wrap items-center justify-end gap-3">
        <Button variant="ghost" onClick={() => setQ({})}>
          {t("wave.setup.reset")}
        </Button>
        <Button onClick={() => onPlay(q)}>{t("wave.setup.play")}</Button>
      </div>
    </Dialog>
  );
}
