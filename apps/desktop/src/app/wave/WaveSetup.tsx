import { isTauri } from "@tauri-apps/api/core";
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
  Users,
  Wand2,
  Waves,
  Zap,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type WaveCount, type WaveQueryIpc } from "@/shared/ipc/bindings";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { Slider } from "@/shared/ui/Slider";
import { NoveltySlider } from "./NoveltySlider";
import { WaveExpert } from "./WaveExpert";

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

type Mode = "normal" | "sleep" | "focus" | "for_all";
const MODES: [Mode, LucideIcon][] = [
  ["normal", Waves],
  ["sleep", Moon],
  ["focus", Briefcase],
  ["for_all", Users],
];
const INFLUENCES = ["season", "context", "events"] as const;
/** The tempo range's ends, BPM. */
const TEMPO: [number, number] = [60, 200];

/** A 0…1 slider with its label and value (the influence strength 0…2). */
function Range({
  label,
  value,
  text,
  onCommit,
}: {
  label: string;
  value: number;
  text: string;
  onCommit: (v: number) => void;
}) {
  return (
    <div className="flex items-center gap-3">
      <span className="t-body w-40 shrink-0 text-text-primary">{label}</span>
      <Slider value={value} onCommit={onCommit} label={label} valueText={text} className="flex-1" />
      <span className="t-caption w-12 shrink-0 text-right text-text-secondary">{text}</span>
    </div>
  );
}

/** A range as two sliders, «от» and «до» (energy, tempo). */
function Span({
  label,
  lo,
  hi,
  min,
  max,
  text,
  onChange,
}: {
  label: string;
  lo: number;
  hi: number;
  min: number;
  max: number;
  text: (v: number) => string;
  onChange: (lo: number, hi: number) => void;
}) {
  const { t } = useTranslation("shell");
  const at = (v: number) => (v - min) / (max - min);
  const of = (x: number) => min + x * (max - min);
  return (
    <div className="flex flex-col gap-2">
      <span className="t-body text-text-primary">
        {t("wave.setup.range", { what: label, lo: text(lo), hi: text(hi) })}
      </span>
      <div className="flex items-center gap-3">
        <Slider
          value={at(lo)}
          onCommit={(x) => onChange(Math.min(of(x), hi), hi)}
          label={t("wave.setup.from", { what: label })}
          valueText={text(lo)}
          className="flex-1"
        />
        <Slider
          value={at(hi)}
          onCommit={(x) => onChange(lo, Math.max(of(x), lo))}
          label={t("wave.setup.to", { what: label })}
          valueText={text(hi)}
          className="flex-1"
        />
      </div>
    </div>
  );
}

/** How many blocks narrow the wave (the button's badge). */
export function setupCount(q: WaveQueryIpc | null): number {
  if (!q) return 0;
  const multi = MULTI.filter((m) => (q[m.block]?.length ?? 0) > 0).length;
  return (
    multi +
    (q.daypart != null ? 1 : 0) +
    (q.weather != null ? 1 : 0) +
    (q.season && q.season !== "auto" ? 1 : 0) +
    (q.influence?.some((v) => v != null && v !== 1) ? 1 : 0) +
    (q.energy ? 1 : 0) +
    (q.tempo ? 1 : 0) +
    ((q.taste ?? 1) !== 1 || (q.excludeGenres?.length ?? 0) > 0 || q.artistSpacing != null ? 1 : 0)
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
  onForAll,
  onSave,
}: {
  initial: WaveQueryIpc | null;
  onClose: () => void;
  /** «Слушать» with this setup (saved by the caller). */
  onPlay: (q: WaveQueryIpc) => void;
  onSleepWave: () => void;
  onFocus: () => void;
  onForAll?: () => void;
  /** «Сохранить как свою волну» (T5.17). */
  onSave?: (q: WaveQueryIpc) => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [q, setQ] = useState<WaveQueryIpc>(initial ?? {});
  // «Режим» (FR-LEARN-011): which wave «Слушать» starts.
  const [mode, setMode] = useState<Mode>("normal");
  // «Подходит N треков» (T5.7, `06` §3): asked again 200 ms after the last press.
  const [count, setCount] = useState<WaveCount | null>(null);
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    const id = window.setTimeout(() => {
      commands
        .waveQueryCount(q)
        .then((r) => {
          if (alive && r.status === "ok") setCount(r.data);
        })
        .catch(() => {});
    }, 200);
    return () => {
      alive = false;
      window.clearTimeout(id);
    };
  }, [q]);
  const toggle = (block: Multi, key: string) =>
    setQ((cur) => {
      const list = cur[block] ?? [];
      return {
        ...cur,
        [block]: list.includes(key) ? list.filter((k) => k !== key) : [...list, key],
      };
    });
  const byFrequency = (q.frequency?.length ?? 0) > 0;
  const play = () => {
    if (mode === "normal") return onPlay(q);
    if (mode === "sleep") onSleepWave();
    else if (mode === "focus") onFocus();
    else onForAll?.();
    onClose();
  };
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
          <h3 className="t-card-title text-text-primary">{t("wave.setup.block.mode")}</h3>
          <div className="flex flex-wrap gap-2">
            {MODES.filter(([k]) => k !== "for_all" || onForAll).map(([key, icon]) => (
              <Tile
                key={key}
                icon={icon}
                label={t(`wave.setup.mode.${key}`)}
                on={mode === key}
                onClick={() => setMode(key)}
              />
            ))}
          </div>
        </section>
        <section className="flex flex-col gap-3">
          <h3 className="t-card-title text-text-primary">{t("wave.setup.block.novelty")}</h3>
          <NoveltySlider disabled={byFrequency} />
          {byFrequency && (
            <span className="t-caption text-text-secondary">
              {t("wave.setup.noveltyByFrequency")}
            </span>
          )}
        </section>
        <section className="flex flex-col gap-3">
          <h3 className="t-card-title text-text-primary">{t("wave.setup.block.influence")}</h3>
          {INFLUENCES.map((key, i) => (
            <Range
              key={key}
              label={t(`wave.setup.influence.${key}`)}
              value={(q.influence?.[i] ?? 1) / 2}
              text={t("wave.setup.times", { value: (q.influence?.[i] ?? 1).toFixed(1) })}
              onCommit={(v) =>
                setQ((c) => {
                  const inf: [number, number, number] = [
                    c.influence?.[0] ?? 1,
                    c.influence?.[1] ?? 1,
                    c.influence?.[2] ?? 1,
                  ];
                  inf[i] = Math.round(v * 20) / 10;
                  return { ...c, influence: inf };
                })
              }
            />
          ))}
        </section>
        <section className="flex flex-col gap-3">
          <h3 className="t-card-title text-text-primary">{t("wave.setup.block.ranges")}</h3>
          <Span
            label={t("wave.setup.energy")}
            lo={q.energy?.[0] ?? 0}
            hi={q.energy?.[1] ?? 1}
            min={0}
            max={1}
            text={(v) => String(Math.round(v * 100))}
            onChange={(lo, hi) =>
              setQ((c) => ({ ...c, energy: lo <= 0 && hi >= 1 ? null : [lo, hi] }))
            }
          />
          <Span
            label={t("wave.setup.tempo")}
            lo={q.tempo?.[0] ?? TEMPO[0]}
            hi={q.tempo?.[1] ?? TEMPO[1]}
            min={TEMPO[0]}
            max={TEMPO[1]}
            text={(v) => t("wave.setup.bpm", { value: Math.round(v) })}
            onChange={(lo, hi) =>
              setQ((c) => ({
                ...c,
                tempo: lo <= TEMPO[0] && hi >= TEMPO[1] ? null : [Math.round(lo), Math.round(hi)],
              }))
            }
          />
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
        <WaveExpert q={q} setQ={setQ} />
      </div>
      <div className="flex flex-wrap items-center justify-end gap-3">
        {count && (
          <span data-testid="wave-count" className="t-caption mr-auto text-text-secondary">
            {count.relaxed
              ? t("wave.setup.countRelaxed", { count: count.tracks })
              : t("wave.setup.count", { count: count.tracks })}
          </span>
        )}
        <Button
          variant="ghost"
          onClick={() => {
            setQ({});
            setMode("normal");
          }}
        >
          {t("wave.setup.reset")}
        </Button>
        {onSave && (
          <Button variant="secondary" disabled={setupCount(q) === 0} onClick={() => onSave(q)}>
            {t("wave.mine.saveAs")}
          </Button>
        )}
        <Button onClick={play}>{t("wave.setup.play")}</Button>
      </div>
    </Dialog>
  );
}
