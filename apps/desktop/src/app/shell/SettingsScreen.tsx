import { CalendarDays, Check, Gauge, Plus } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CollectionSummary } from "@/app/library/CollectionScreen";
import { mb } from "@/app/library/TrackCard";
import type { SignalInfo } from "@/shared/ipc/bindings";
import { type City, cityLocate, citySearch, useWeather } from "@/shared/ipc/context";
import { duplicatesCount } from "@/shared/ipc/duplicates";
import {
  analysisProgress,
  type NeverEntry,
  never,
  neverList,
  resetLearning,
} from "@/shared/ipc/feedback";
import type { LibraryStatus } from "@/shared/ipc/library";
import { organizeLibrary } from "@/shared/ipc/library";
import { perfHardware } from "@/shared/ipc/perf";
import { getSnapshot, onSnapshot } from "@/shared/ipc/playback";
import { getSettings, onSettingsChanged, patchSettings } from "@/shared/ipc/settings";
import { sfx } from "@/shared/ipc/sfx";
import { sleepCaps } from "@/shared/ipc/sleep";
import { clearOldLogs, type StorageInfo, storageInfo } from "@/shared/ipc/storage";
import {
  autostartAvailable,
  type HotkeyAction,
  type HotkeyStatus,
  hotkeysStatus,
  setAutostart,
} from "@/shared/ipc/window";
import { inkOn } from "@/shared/lib/color";
import { accelFromEvent } from "@/shared/lib/hotkeys";
import { cn } from "@/shared/lib/utils";
import { benchmark, score } from "@/shared/theme/autodetect";
import { applyPerformance, profile } from "@/shared/theme/performance";
import { ACCENT_SLUGS, type AccentSource, isPalette, type ThemePref } from "@/shared/theme/theme";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Dialog } from "@/shared/ui/Dialog";
import { notify } from "@/shared/ui/notify";
import { Slider } from "@/shared/ui/Slider";
import { Switch } from "@/shared/ui/Switch";
import { FX_TYPES, type FxType, isFxType } from "@/shared/viz/fxPlan";
import EQ_PRESETS_JSON from "../../../../../assets/eq/presets.json";

type Props = {
  theme: ThemePref;
  /** `null` — the seasonal theme's own colour (T4.18). */
  accent: AccentSource | null;
  /** «Тема по сезону» is on: today's seed for the season swatch. */
  seasonSeed?: string | null;
  onTheme: (t: ThemePref) => void;
  onAccent: (a: AccentSource | null) => void;
  /** «Хранилище» (FR-DATA-004, FR-ONB-003): library folder and counters. */
  library?: LibraryStatus;
  onChooseLibrary?: () => void;
  /** FR-ONB-002/003: the setup and the tips again. */
  onReplaySetup?: () => void;
  onReplayTips?: () => void;
  /** «Дубли» (T3.5). */
  onShowDuplicates?: () => void;
};

const THEMES: { id: ThemePref; key: string }[] = [
  { id: "system", key: "settings.themeSystem" },
  { id: "dark", key: "settings.themeDark" },
  { id: "light", key: "settings.themeLight" },
  { id: "ember", key: "settings.themeEmber" },
  { id: "mono", key: "settings.themeMono" },
];

/** Settings → Внешний вид: theme + AccentPicker (DESIGN.md §9), applied instantly, no «Сохранить». */
export function SettingsScreen({
  theme,
  accent,
  seasonSeed,
  onTheme,
  onAccent,
  library,
  onChooseLibrary,
  onReplaySetup,
  onReplayTips,
  onShowDuplicates,
}: Props) {
  const { t } = useTranslation("shell");

  return (
    <section className="flex max-w-3xl flex-col gap-12">
      <h1 className="t-page-title text-primary-text">{t("settings.title")}</h1>

      <div className="flex flex-col gap-4">
        <h2 className="t-shelf-title text-primary-text">{t("settings.appearance")}</h2>

        <div className="flex flex-col gap-3 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
          <span className="t-card-title text-text-primary">{t("settings.theme")}</span>
          <ThemeChips theme={theme} onTheme={onTheme} />
        </div>

        <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
          <span className="t-card-title text-text-primary">{t("settings.color")}</span>
          {isPalette(theme) && (
            <span className="t-caption text-text-secondary">{t("settings.accentByTheme")}</span>
          )}
          <AccentSwatches
            theme={theme}
            accent={accent}
            seasonSeed={seasonSeed}
            onAccent={onAccent}
          />
          <SeasonSwitch />

          <div className="mt-2 flex items-center gap-4 border-t border-separator pt-4">
            <span className="t-caption text-text-tertiary">{t("settings.preview")}</span>
            <span className="t-badge rounded-full bg-primary px-2 py-0.5 text-on-primary">
              {t("settings.badge")}
            </span>
            <button
              type="button"
              className="t-body-strong h-(--button-height) rounded-full bg-accent-fill px-5 text-accent-on"
            >
              {t("settings.previewButton")}
            </button>
            <div className="h-1 w-40 rounded-full bg-inactive">
              <div className="h-1 w-2/3 rounded-full bg-accent-text" />
            </div>
          </div>
        </div>
      </div>

      {library && (
        <div className="flex flex-col gap-2">
          <h2 className="t-shelf-title text-primary-text">{t("settingsStorage.title")}</h2>
          <div className="flex flex-col gap-3 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
            <span className="t-body-strong text-text-primary">{t("settingsStorage.folder")}</span>
            <span className="t-body break-all text-text-secondary">
              {library.root ?? t("settingsStorage.notChosen")}
            </span>
            {library.root && <CollectionSummary status={library} />}
            {library.root && <ServiceData />}
            {library.root && onShowDuplicates && <DuplicatesLine onShow={onShowDuplicates} />}
            {library.root && <OrganizeLine />}
            {!library.root && onChooseLibrary && (
              <button
                type="button"
                onClick={onChooseLibrary}
                className="t-body-strong w-fit text-accent-text"
              >
                {t("settingsStorage.change")}
              </button>
            )}
          </div>
        </div>
      )}

      <SoundSection />

      <EqSection />

      <TransitionsSection />

      <DuckSection />

      <SfxSection />

      <NetworkSection />

      <PerformanceSection />

      <StartupSection />

      <HotkeysSection />

      <WaveLearningSection />
      <PlaceSection />

      <VizSection />

      <FxSection />

      <SleepSection />

      <FocusSection />

      <NeverSection />

      <div className="flex flex-col gap-2">
        <h2 className="t-shelf-title text-primary-text">{t("settings.about")}</h2>
        <p className="t-body text-text-secondary">
          {t("settings.version", { version: __APP_VERSION__ })}
        </p>
        <p className="t-caption text-text-tertiary">{t("updates.auto")}</p>
        <div className="flex flex-wrap gap-3 pt-2">
          {onReplayTips && (
            <Button variant="secondary" onClick={onReplayTips}>
              {t("tips.again")}
            </Button>
          )}
          {onReplaySetup && (
            <Button variant="ghost" onClick={onReplaySetup}>
              {t("onboarding.again")}
            </Button>
          )}
        </div>
      </div>
    </section>
  );
}

/** «Запуск» (FR-SHELL-007): start with Windows, minimized to the tray. Optimistic: the switch
 * moves at once and rolls back if Windows refuses. */
function StartupSection() {
  const { t } = useTranslation("shell");
  const [available, setAvailable] = useState(false);
  const [on, setOn] = useState(false);

  useEffect(() => {
    autostartAvailable().then(setAvailable);
    getSettings().then((s) => {
      const w = s.window as { autostart?: boolean } | undefined;
      setOn(Boolean(w?.autostart));
    });
  }, []);

  const change = (next: boolean) => {
    setOn(next);
    setAutostart(next)
      .then(setOn)
      .catch(() => setOn(!next));
  };

  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsStartup.title")}</h2>
      <div className="flex items-center gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <span className="t-body-strong text-text-primary">{t("settingsStartup.autostart")}</span>
          <span className="t-caption text-text-secondary">
            {available
              ? t("settingsStartup.autostartHint")
              : t("settingsStartup.autostartUnavailable")}
          </span>
        </div>
        <Switch
          checked={on}
          onChange={change}
          label={t("settingsStartup.autostart")}
          disabled={!available}
        />
      </div>
    </div>
  );
}

/** Defaults from settings (docs/03 §6): a switched-off action gets its default back on. */
const HOTKEYS: { action: HotkeyAction; key: string; def: string }[] = [
  { action: "playPause", key: "play_pause", def: "Ctrl+Alt+Space" },
  { action: "next", key: "next", def: "Ctrl+Alt+Right" },
  { action: "prev", key: "prev", def: "Ctrl+Alt+Left" },
  { action: "volumeUp", key: "volume_up", def: "Ctrl+Alt+Up" },
  { action: "volumeDown", key: "volume_down", def: "Ctrl+Alt+Down" },
  { action: "mute", key: "mute", def: "Ctrl+Alt+M" },
];

/** «Горячие клавиши» (FR-WIN-001): combination, on/off, «занято другой программой». */
function HotkeysSection() {
  const { t } = useTranslation("shell");
  const [status, setStatus] = useState<HotkeyStatus[]>([]);
  const [recording, setRecording] = useState<string | null>(null);
  const [problem, setProblem] = useState<{ action: string; text: string } | null>(null);
  const reload = () => {
    hotkeysStatus().then(setStatus);
  };
  useEffect(reload, []);

  const save = (key: string, accel: string | null) => {
    patchSettings({ hotkeys: { [key]: accel } }).then(reload, reload);
  };

  // Recording a new combination (FR-WIN-001 editor): the next usable key press wins.
  useEffect(() => {
    if (!recording) return;
    const h = HOTKEYS.find((x) => x.key === recording);
    const down = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(null);
        setProblem(null);
        return;
      }
      const accel = accelFromEvent(e);
      if (!accel) {
        if (!["Control", "Alt", "Shift", "Meta"].includes(e.key) && h) {
          setProblem({ action: h.action, text: t("settingsHotkeys.needModifier") });
        }
        return;
      }
      const clash = status.find((s) => s.accel === accel && s.action !== h?.action);
      if (clash && h) {
        setProblem({
          action: h.action,
          text: t("settingsHotkeys.duplicate", {
            action: t(`settingsHotkeys.action.${clash.action}`),
          }),
        });
        return;
      }
      setRecording(null);
      setProblem(null);
      const refresh = () => hotkeysStatus().then(setStatus);
      patchSettings({ hotkeys: { [recording]: accel } }).then(refresh, refresh);
    };
    window.addEventListener("keydown", down, true);
    return () => window.removeEventListener("keydown", down, true);
  }, [recording, status, t]);

  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsHotkeys.title")}</h2>
      <p className="t-caption text-text-secondary">{t("settingsHotkeys.hint")}</p>
      <div className="flex flex-col divide-y divide-border rounded-(--radius-xl) border border-border bg-bg-raised">
        {HOTKEYS.map((h) => {
          const st = status.find((s) => s.action === h.action);
          const label = t(`settingsHotkeys.action.${h.action}`);
          const isRecording = recording === h.key;
          return (
            <div key={h.action} className="flex items-center gap-4 px-6 py-3">
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="t-body text-text-primary">{label}</span>
                {problem?.action === h.action ? (
                  <span className="t-caption text-danger">{problem.text}</span>
                ) : (
                  st &&
                  !st.registered && (
                    <span className="t-caption text-danger">{t("settingsHotkeys.taken")}</span>
                  )
                )}
              </div>
              <button
                type="button"
                aria-label={t("settingsHotkeys.change", { action: label })}
                aria-pressed={isRecording}
                disabled={!st}
                onClick={() => {
                  setProblem(null);
                  setRecording(isRecording ? null : h.key);
                }}
                className={cn(
                  "t-caption rounded-(--radius-md) border px-2 py-1 disabled:opacity-40",
                  isRecording
                    ? "border-accent-text text-accent-text"
                    : "border-border text-text-secondary hover:bg-bg-hover",
                )}
              >
                {isRecording ? t("settingsHotkeys.record") : (st?.accel ?? h.def)}
              </button>
              <Switch
                checked={Boolean(st)}
                onChange={(on) => save(h.key, on ? (st?.accel ?? h.def) : null)}
                label={label}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}

/** «Производительность» (T2.3, FR-PERF-001/002): five presets + point switches over them. */
export function PerformanceSection() {
  const { t } = useTranslation("shell");
  const [level, setLevel] = useState(3);
  const [overrides, setOverrides] = useState<Record<string, unknown>>({});
  const [autoDegrade, setAutoDegrade] = useState(true);
  useEffect(() => {
    let alive = true;
    let off: (() => void) | undefined;
    const load = () =>
      getSettings().then((doc) => {
        if (!alive) return;
        const p = (doc.performance ?? {}) as {
          level?: number;
          overrides?: Record<string, unknown>;
          auto_degrade?: boolean;
        };
        setLevel(p.level ?? 3);
        setOverrides(p.overrides ?? {});
        setAutoDegrade(p.auto_degrade !== false);
      });
    void load();
    // The level may drop by itself (T4.16): show it.
    onSettingsChanged(load).then((u) => {
      if (alive) off = u;
      else u();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, []);
  const levels = t("settingsPerf.level", { returnObjects: true }) as unknown as string[];
  const eff = profile(level, overrides);
  // Choosing a preset clears the point switches: the preset is what you asked for.
  const choose = (l: number) => {
    setLevel(l);
    setOverrides({});
    applyPerformance(profile(l, {}));
    // A manual choice is the way back from an auto-degrade (T4.16): nothing sticks any more.
    patchSettings({ performance: { level: l, overrides: {}, degraded_from: null } }).catch(
      () => {},
    );
  };
  const [detecting, setDetecting] = useState(false);
  // FR-PERF-003: hardware + a 1.5 s WebGL benchmark → a level, applied like a manual choice.
  const detect = () => {
    setDetecting(true);
    Promise.all([perfHardware(), benchmark()])
      .then(([hw, median]) => {
        const l = score(hw, median);
        choose(l);
        notify(t("settingsPerf.detected", { level: levels[l] ?? "" }));
      })
      .catch(() => notify(t("settingsPerf.detectFailed")))
      .finally(() => setDetecting(false));
  };
  const flip = (key: string, on: boolean) => {
    const next = { ...overrides, [key]: on };
    setOverrides(next);
    applyPerformance(profile(level, next));
    patchSettings({ performance: { overrides: next } }).catch(() => {});
  };
  const switches: { key: string; label: string; on: boolean }[] = [
    { key: "ui_animations", label: t("settingsPerf.uiAnimations"), on: eff.uiAnimations },
    { key: "list_animations", label: t("settingsPerf.listAnimations"), on: eff.listAnimations },
    { key: "blur", label: t("settingsPerf.blur"), on: eff.blur },
    // FR-VIZ-008: pause when unseen, lower on battery — on unless switched off.
    {
      key: "viz_power",
      label: t("settingsPerf.vizPower"),
      on: overrides.viz_power !== false,
    },
  ];
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsPerf.title")}</h2>
      <p className="t-caption text-text-secondary">{t("settingsPerf.hint")}</p>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <fieldset
          className="m-0 flex flex-wrap gap-2 border-0 p-0"
          aria-label={t("settingsPerf.title")}
        >
          {levels.map((name, i) => (
            <Chip key={name} active={level === i} onClick={() => choose(i)}>
              {name}
            </Chip>
          ))}
        </fieldset>
        <Button variant="secondary" disabled={detecting} onClick={detect} className="self-start">
          <Gauge size={16} strokeWidth={1.5} />
          {detecting ? t("settingsPerf.detecting") : t("settingsPerf.detect")}
        </Button>
        <div className="flex flex-col divide-y divide-border">
          <div className="flex items-center gap-4 py-3">
            <span className="t-body min-w-0 flex-1 text-text-primary">
              {t("settingsPerf.autoDegrade")}
            </span>
            <Switch
              checked={autoDegrade}
              label={t("settingsPerf.autoDegrade")}
              onChange={(on) => {
                setAutoDegrade(on);
                patchSettings({ performance: { auto_degrade: on } }).catch(() =>
                  setAutoDegrade(!on),
                );
              }}
            />
          </div>
          {switches.map((s) => (
            <div key={s.key} className="flex items-center gap-4 py-3">
              <span className="t-body min-w-0 flex-1 text-text-primary">{s.label}</span>
              <Switch checked={s.on} onChange={(on) => flip(s.key, on)} label={s.label} />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

/** «Никогда в волне» (FR-WENG-009): what was excluded, and «Вернуть». Optimistic. */
function NeverSection() {
  const { t } = useTranslation("shell");
  const [items, setItems] = useState<NeverEntry[]>([]);
  useEffect(() => {
    neverList()
      .then(setItems)
      .catch(() => setItems([]));
  }, []);
  const back = (e: NeverEntry) => {
    setItems((l) => l.filter((x) => !(x.scope === e.scope && x.id === e.id)));
    never(e.scope, e.id, false).catch(() => {
      setItems((l) => [...l, e]);
      notify(t("wave.failed"));
    });
  };
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsNever.title")}</h2>
      <p className="t-caption text-text-secondary">{t("settingsNever.hint")}</p>
      <div className="flex flex-col divide-y divide-border rounded-(--radius-xl) border border-border bg-bg-raised px-6">
        {items.length === 0 ? (
          <p className="t-body py-4 text-text-secondary">{t("settingsNever.empty")}</p>
        ) : (
          items.map((e) => (
            <div key={`${e.scope}:${e.id}`} className="flex items-center gap-4 py-3">
              <div className="min-w-0 flex-1">
                <div className="t-body truncate text-text-primary">{e.title}</div>
                <div className="t-caption truncate text-text-secondary">
                  {e.scope === "artist" ? t("settingsNever.artist") : e.subtitle}
                </div>
              </div>
              <Button variant="secondary" onClick={() => back(e)}>
                {t("settingsNever.back")}
              </Button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

/** FR-DATA-004: what Сиртон keeps besides the music, and «Удалить старые журналы». */
function ServiceData() {
  const { t } = useTranslation("shell");
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const load = useCallback(() => {
    storageInfo()
      .then(setInfo)
      .catch(() => setInfo(null));
  }, []);
  useEffect(load, [load]);
  if (!info) return null;
  const mbOf = (b: number | null) => mb(b ?? 0);
  return (
    <div className="flex flex-col gap-2 border-t border-separator pt-3">
      <span className="t-body-strong text-text-primary">{t("settingsStorage.service")}</span>
      <dl className="t-caption grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-text-secondary">
        <dt>{t("settingsStorage.database")}</dt>
        <dd className="text-text-primary">
          {t("trackCard.mb", { value: mbOf(info.databaseBytes) })}
        </dd>
        <dt>{t("settingsStorage.covers")}</dt>
        <dd className="text-text-primary">
          {t("settingsStorage.coversValue", {
            value: mbOf(info.coversBytes),
            count: info.coversFiles,
          })}
        </dd>
        <dt>{t("settingsStorage.logs")}</dt>
        <dd className="text-text-primary">{t("trackCard.mb", { value: mbOf(info.logsBytes) })}</dd>
      </dl>
      {info.oldLogs > 0 && (
        <Button
          variant="secondary"
          className="self-start"
          onClick={() =>
            clearOldLogs()
              .then((n) => {
                notify(t("settingsStorage.logsCleared", { count: n }));
                load();
              })
              .catch(() => notify(t("wave.failed")))
          }
        >
          {t("settingsStorage.clearLogs", { count: info.oldLogs })}
        </Button>
      )}
    </div>
  );
}

/** Theme chips (Settings and the first-start setup, FR-ONB-003). */
export function ThemeChips({
  theme,
  onTheme,
}: {
  theme: ThemePref;
  onTheme: (t: ThemePref) => void;
}) {
  const { t } = useTranslation("shell");
  return (
    <fieldset className="m-0 border-0 p-0 flex flex-wrap gap-2" aria-label={t("settings.theme")}>
      {THEMES.map((it) => (
        <button
          key={it.id}
          type="button"
          aria-pressed={theme === it.id}
          onClick={() => onTheme(it.id)}
          className={cn(
            "t-menu h-(--chip-height) rounded-full px-3",
            theme === it.id
              ? "bg-primary text-on-primary"
              : "border border-border text-text-primary hover:bg-bg-hover",
          )}
        >
          {t(it.key)}
        </button>
      ))}
    </fieldset>
  );
}

/** AccentPicker swatches (DESIGN.md §9): Sir.kov, 16 colours, «Свой». */
export function AccentSwatches({
  theme,
  accent,
  seasonSeed,
  onAccent,
}: {
  theme: ThemePref;
  /** `null` — the seasonal theme's own colour is chosen. */
  accent: AccentSource | null;
  /** «Тема по сезону» on (T4.18, FR-UI-003): the season swatch comes first; a pick is the
   * current theme's own accent (Q-15), the season swatch returns to the theme's colour. */
  seasonSeed?: string | null | undefined;
  onAccent: (a: AccentSource | null) => void;
}) {
  const { t } = useTranslation("shell");
  const customHex = accent?.kind === "custom" ? accent.hex : "#3b82f6";
  return (
    <fieldset
      disabled={isPalette(theme)}
      className="m-0 border-0 p-0 flex flex-wrap gap-3 disabled:opacity-40"
      aria-label={t("settings.color")}
    >
      {seasonSeed && (
        <button
          type="button"
          aria-pressed={accent === null}
          aria-label={t("accent.season")}
          title={t("accent.season")}
          onClick={() => onAccent(null)}
          className={cn(
            "relative flex size-8 items-center justify-center rounded-full",
            accent === null && "ring-2 ring-text-primary ring-offset-[3px] ring-offset-bg-raised",
          )}
          style={{ background: seasonSeed }}
        >
          <CalendarDays size={16} strokeWidth={1.5} style={{ color: inkOn(seasonSeed) }} />
        </button>
      )}
      <button
        type="button"
        aria-pressed={accent?.kind === "auto"}
        aria-label={t("accent.auto")}
        title={t("accent.autoHint")}
        onClick={() => onAccent({ kind: "auto" })}
        className={cn(
          "relative flex size-8 items-center justify-center rounded-full",
          accent?.kind === "auto" &&
            "ring-2 ring-text-primary ring-offset-[3px] ring-offset-bg-raised",
        )}
        style={{
          // DESIGN.md §9 AccentPicker: «Авто» — four sectors, first in the row.
          background:
            "conic-gradient(var(--accent-blue-fill) 0 25%, var(--accent-yellow-fill) 0 50%, var(--accent-red-fill) 0 75%, var(--accent-sirkov-fill) 0)",
        }}
      />
      {ACCENT_SLUGS.map((slug) => {
        const selected = accent?.kind === "preset" && accent.slug === slug;
        return (
          <button
            key={slug}
            type="button"
            aria-pressed={selected}
            aria-label={t(`accent.${slug}`)}
            title={t(`accent.${slug}`)}
            onClick={() => onAccent({ kind: "preset", slug })}
            className={cn(
              "relative flex size-8 items-center justify-center rounded-full",
              selected && "ring-2 ring-text-primary ring-offset-[3px] ring-offset-bg-raised",
            )}
            style={
              slug === "sirkov"
                ? {
                    background: `linear-gradient(135deg, var(--accent-${slug}-primary) 50%, var(--accent-${slug}-fill) 50%)`,
                  }
                : { background: `var(--accent-${slug}-fill)` }
            }
          >
            {selected && (
              <Check size={16} strokeWidth={2} style={{ color: `var(--accent-${slug}-on)` }} />
            )}
          </button>
        );
      })}
      <label
        title={t("settings.custom")}
        className={cn(
          "relative flex size-8 cursor-pointer items-center justify-center rounded-full border border-border text-icon",
          accent?.kind === "custom" &&
            "ring-2 ring-text-primary ring-offset-[3px] ring-offset-bg-raised",
        )}
        style={accent?.kind === "custom" ? { background: customHex } : undefined}
      >
        <Plus size={16} strokeWidth={1.5} />
        <input
          type="color"
          aria-label={t("settings.custom")}
          value={customHex}
          onChange={(e) => onAccent({ kind: "custom", hex: e.target.value })}
          className="absolute inset-0 cursor-pointer opacity-0"
        />
      </label>
    </fieldset>
  );
}

/** «Тема по сезону» (FR-ONB-001 step 2, FR-THEME-002, T4.18): the accent follows the season
 * (the shell listens to the settings and applies it). */
export function SeasonSwitch() {
  const { t } = useTranslation("shell");
  const [on, setOn] = useState(false);
  useEffect(() => {
    getSettings().then((s) => {
      const a = s.appearance as { seasonal_theme?: boolean } | undefined;
      setOn(Boolean(a?.seasonal_theme));
    });
  }, []);
  return (
    <div className="flex items-center gap-4 border-t border-separator pt-3">
      <div className="min-w-0 flex-1">
        <div className="t-body text-text-primary">{t("settings.seasonal")}</div>
        <div className="t-caption text-text-secondary">{t("settings.seasonalHint")}</div>
      </div>
      <Switch
        checked={on}
        label={t("settings.seasonal")}
        onChange={(v) => {
          setOn(v);
          patchSettings({ appearance: { seasonal_theme: v } }).catch(() => setOn(!v));
        }}
      />
    </div>
  );
}

const EQ_PRESETS = EQ_PRESETS_JSON as {
  bands_hz: number[];
  presets: { id: string; bands_db: number[] }[];
};

/** The three targets (Spotify Quiet / Normal / Loud, `01b` F-02). */
const TARGETS = [-19, -14, -11] as const;

/**
 * «Звук» (T4.11, FR-PLAY-008): loudness normalization — on/off, «Тихо / Обычно / Громко»,
 * album gain while an album plays in order. Applied at once (the engine ramps the gain).
 */
/** «Тракт: Bit-perfect · 48 кГц» (T4.13, FR-PLAY-002, `04` §8), live from the player. */
function SignalLine() {
  const { t } = useTranslation("shell");
  const [sig, setSig] = useState<SignalInfo | null>(null);
  useEffect(() => {
    let alive = true;
    let off: (() => void) | undefined;
    getSnapshot().then((s) => alive && setSig(s.signal));
    onSnapshot((s) => alive && setSig(s.signal)).then((u) => {
      if (alive) off = u;
      else u();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, []);
  if (!sig) return null;
  const khz = (hz: number) => t("settingsSound.khz", { v: (hz / 1000).toLocaleString("ru") });
  const text =
    sig.kind === "bitperfect"
      ? t("settingsSound.signal.bitperfect", { rate: khz(sig.deviceRate) })
      : sig.kind === "resampled"
        ? t("settingsSound.signal.resampled", {
            from: khz(sig.sourceRate),
            to: khz(sig.deviceRate),
          })
        : t("settingsSound.signal.dsp");
  return (
    <div className="t-caption text-text-secondary" data-testid="signal-path">
      {text}
    </div>
  );
}

function SoundSection() {
  const { t } = useTranslation("shell");
  const [n, setN] = useState({ enabled: true, target: -14, album: true });
  useEffect(() => {
    getSettings().then((s) => {
      const v = ((s.playback ?? {}) as { normalization?: Record<string, unknown> }).normalization;
      setN({
        enabled: v?.enabled !== false,
        target: typeof v?.target_lufs === "number" ? v.target_lufs : -14,
        album: v?.album_mode !== "off",
      });
    });
  }, []);
  const save = (next: typeof n) => {
    const prev = n;
    setN(next);
    patchSettings({
      playback: {
        normalization: {
          enabled: next.enabled,
          target_lufs: next.target,
          album_mode: next.album ? "auto" : "off",
        },
      },
    }).catch(() => setN(prev));
  };
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsSound.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <SignalLine />
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <div className="t-body text-text-primary">{t("settingsSound.normalize")}</div>
            <div className="t-caption text-text-secondary">{t("settingsSound.normalizeHint")}</div>
          </div>
          <Switch
            checked={n.enabled}
            label={t("settingsSound.normalize")}
            onChange={(v) => save({ ...n, enabled: v })}
          />
        </div>
        {n.enabled && (
          <>
            <fieldset
              className="m-0 flex flex-wrap gap-2 border-0 p-0"
              aria-label={t("settingsSound.level")}
            >
              {TARGETS.map((v, i) => (
                <Chip key={v} active={n.target === v} onClick={() => save({ ...n, target: v })}>
                  {t(`settingsSound.levels.${i}`)}
                </Chip>
              ))}
            </fieldset>
            <div className="flex items-center gap-4">
              <div className="min-w-0 flex-1">
                <div className="t-body text-text-primary">{t("settingsSound.album")}</div>
                <div className="t-caption text-text-secondary">{t("settingsSound.albumHint")}</div>
              </div>
              <Switch
                checked={n.album}
                label={t("settingsSound.album")}
                onChange={(v) => save({ ...n, album: v })}
              />
            </div>
          </>
        )}
      </div>
    </div>
  );
}

type EqState = { enabled: boolean; preset: string; preamp: number; bands: number[] };
const EQ_FLAT: EqState = { enabled: false, preset: "flat", preamp: 0, bands: Array(10).fill(0) };
/** Slider 0…1 ↔ −12…+12 dB, in 0.5 dB steps. */
const toDb = (v: number) => Math.round((v * 24 - 12) * 2) / 2;
const fromDb = (db: number) => (db + 12) / 24;

/**
 * «Эквалайзер» (T4.12, FR-PLAY-009, `04` §6 — Winamp/VLC 10 bands + preamp, Apple's preset
 * names): on/off (off is a true bypass), presets, 10 bands and the preamp, ±12 dB. Moving a band
 * makes the preset «Свой». The engine glides to new values in 20 ms.
 */
/** The preset an edit started from (an old «Свой» — the flat one). */
function eqBase(eq: EqState) {
  return (
    EQ_PRESETS.presets.find((p) => p.id === eq.preset) ??
    EQ_PRESETS.presets.find((p) => p.id === "flat") ??
    EQ_PRESETS.presets[0] ?? { id: "flat", bands_db: [] }
  );
}

/** The bands or the preamp moved off the preset (owner, 2026-10-20: «Сбросить»). */
function eqEdited(eq: EqState): boolean {
  const base = eqBase(eq);
  return (
    eq.preset === "custom" ||
    eq.preamp !== 0 ||
    eq.bands.some((v, i) => Math.abs(v - (base.bands_db[i] ?? 0)) > 0.01)
  );
}

function EqSection() {
  const { t } = useTranslation("shell");
  const [eq, setEq] = useState<EqState>(EQ_FLAT);
  useEffect(() => {
    getSettings().then((s) => {
      const v = ((s.playback ?? {}) as { eq?: Record<string, unknown> }).eq;
      setEq({
        enabled: v?.enabled === true,
        preset: typeof v?.preset === "string" ? v.preset : "flat",
        preamp: typeof v?.preamp_db === "number" ? v.preamp_db : 0,
        bands: Array.isArray(v?.bands_db) ? (v.bands_db as number[]) : EQ_FLAT.bands,
      });
    });
  }, []);
  const save = (next: EqState) => {
    const prev = eq;
    setEq(next);
    patchSettings({
      playback: {
        eq: {
          enabled: next.enabled,
          preset: next.preset,
          preamp_db: next.preamp,
          bands_db: next.bands,
        },
      },
    }).catch(() => setEq(prev));
  };
  const hz = (f: number) =>
    f >= 1000 ? t("settingsEq.khz", { v: f / 1000 }) : t("settingsEq.hz", { v: f });
  const db = (v: number) => t("settingsEq.db", { v: v > 0 ? `+${v}` : String(v) });
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsEq.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <div className="t-body text-text-primary">{t("settingsEq.enable")}</div>
            <div className="t-caption text-text-secondary">{t("settingsEq.hint")}</div>
          </div>
          <Switch
            checked={eq.enabled}
            label={t("settingsEq.enable")}
            onChange={(v) => save({ ...eq, enabled: v })}
          />
        </div>
        {eq.enabled && (
          <>
            <fieldset
              className="m-0 flex flex-wrap gap-2 border-0 p-0"
              aria-label={t("settingsEq.presets")}
            >
              {EQ_PRESETS.presets.map((p) => (
                <Chip
                  key={p.id}
                  active={eq.preset === p.id}
                  onClick={() => save({ ...eq, preset: p.id, bands: p.bands_db })}
                >
                  {t(`settingsEq.preset.${p.id}`)}
                </Chip>
              ))}
              {eq.preset === "custom" && (
                <Chip active onClick={() => {}}>
                  {t("settingsEq.preset.custom")}
                </Chip>
              )}
            </fieldset>
            {eqEdited(eq) && (
              <div className="flex items-center gap-3">
                <span className="t-caption flex-1 text-text-secondary">
                  {t("settingsEq.edited", {
                    preset: t(`settingsEq.preset.${eqBase(eq).id}`),
                  })}
                </span>
                <Button
                  variant="secondary"
                  onClick={() =>
                    save({
                      ...eq,
                      preset: eqBase(eq).id,
                      preamp: 0,
                      bands: [...eqBase(eq).bands_db],
                    })
                  }
                >
                  {t("settingsEq.reset")}
                </Button>
              </div>
            )}
            <div className="flex flex-col gap-1">
              {[
                { key: 0, label: t("settingsEq.preamp"), value: eq.preamp },
                ...EQ_PRESETS.bands_hz.map((f, i) => ({
                  key: f,
                  label: hz(f),
                  value: eq.bands[i] ?? 0,
                })),
              ].map((row, i) => (
                <div key={row.key} className="flex items-center gap-4">
                  <span className="t-caption w-20 shrink-0 text-text-secondary">{row.label}</span>
                  <Slider
                    value={fromDb(row.value)}
                    step={1 / 48}
                    label={row.label}
                    valueText={db(row.value)}
                    onCommit={(v) => {
                      const d = toDb(v);
                      if (i === 0) save({ ...eq, preamp: d });
                      else {
                        const bands = [...eq.bands];
                        bands[i - 1] = d;
                        // The preset stays as the base: «Сбросить» goes back to it.
                        save({ ...eq, bands });
                      }
                    }}
                    className="flex-1"
                  />
                  <span className="t-caption w-14 shrink-0 text-right text-text-tertiary">
                    {db(row.value)}
                  </span>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/**
 * «Звуки интерфейса» (T4.15, FR-PLAY-016, `06` §8 — Discord/Slack/Telegram have their own switch
 * and volume): played by the engine; on by default, quiet (0.5).
 */
function SfxSection() {
  const { t } = useTranslation("shell");
  const [s, setS] = useState({ enabled: true, volume: 0.5 });
  useEffect(() => {
    getSettings().then((d) => {
      const v = d.sfx as { enabled?: boolean; volume?: number } | undefined;
      setS({ enabled: v?.enabled !== false, volume: v?.volume ?? 0.5 });
    });
  }, []);
  const save = (next: typeof s) => {
    const prev = s;
    setS(next);
    patchSettings({ sfx: next }).catch(() => setS(prev));
  };
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsSfx.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <div className="t-body text-text-primary">{t("settingsSfx.enable")}</div>
            <div className="t-caption text-text-secondary">{t("settingsSfx.hint")}</div>
          </div>
          <Switch
            checked={s.enabled}
            label={t("settingsSfx.enable")}
            onChange={(v) => save({ ...s, enabled: v })}
          />
        </div>
        {s.enabled && (
          <div className="flex items-center gap-4">
            <span className="t-caption w-28 shrink-0 text-text-secondary">
              {t("settingsSfx.volume")}
            </span>
            <Slider
              value={s.volume}
              step={0.05}
              label={t("settingsSfx.volume")}
              valueText={t("settingsDuck.pct", { v: Math.round(s.volume * 100) })}
              onCommit={(v) => {
                save({ ...s, volume: Math.round(v * 20) / 20 });
                sfx("click");
              }}
              className="flex-1"
            />
          </div>
        )}
      </div>
    </div>
  );
}

const DUCK_SENS = ["low", "medium", "high"] as const;
type Duck = {
  enabled: boolean;
  /** Reduction 0…100 % (Discord-style); 75 % ≈ −12 dB. */
  pct: number;
  sensitivity: (typeof DUCK_SENS)[number];
  ignore: string[];
};
const pctToDb = (pct: number) => (pct >= 100 ? -80 : Math.max(-80, 20 * Math.log10(1 - pct / 100)));
const dbToPct = (db: number) => Math.round((1 - 10 ** (db / 20)) * 100);

/**
 * «Приглушение» (T4.14, FR-PLAY-011, `04` §7 — Discord attenuation, Windows communications
 * ducking): Sirtone gets quieter while another app plays sound (≥ 0.7 s, not short notifications),
 * comes back a second after; strength in %, sensitivity, apps never to duck for.
 */
function DuckSection() {
  const { t } = useTranslation("shell");
  const [duck, setDuck] = useState<Duck>({
    enabled: true,
    pct: 75,
    sensitivity: "medium",
    ignore: [],
  });
  const [adding, setAdding] = useState("");
  useEffect(() => {
    getSettings().then((s) => {
      const d = ((s.playback ?? {}) as { duck?: Record<string, unknown> }).duck;
      setDuck({
        enabled: d?.enabled !== false,
        pct: typeof d?.amount_db === "number" ? dbToPct(d.amount_db) : 75,
        sensitivity: DUCK_SENS.includes(d?.sensitivity as Duck["sensitivity"])
          ? (d?.sensitivity as Duck["sensitivity"])
          : "medium",
        ignore: Array.isArray(d?.ignore) ? (d.ignore as string[]) : [],
      });
    });
  }, []);
  const save = (next: Duck) => {
    const prev = duck;
    setDuck(next);
    patchSettings({
      playback: {
        duck: {
          enabled: next.enabled,
          amount_db: pctToDb(next.pct),
          sensitivity: next.sensitivity,
          ignore: next.ignore,
        },
      },
    }).catch(() => setDuck(prev));
  };
  const add = () => {
    const exe = adding.trim().toLowerCase();
    if (!exe) return;
    const name = exe.endsWith(".exe") ? exe : `${exe}.exe`;
    if (!duck.ignore.includes(name)) save({ ...duck, ignore: [...duck.ignore, name] });
    setAdding("");
  };
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsDuck.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <div className="t-body text-text-primary">{t("settingsDuck.enable")}</div>
            <div className="t-caption text-text-secondary">{t("settingsDuck.hint")}</div>
          </div>
          <Switch
            checked={duck.enabled}
            label={t("settingsDuck.enable")}
            onChange={(v) => save({ ...duck, enabled: v })}
          />
        </div>
        {duck.enabled && (
          <>
            <div className="flex items-center gap-4">
              <span className="t-caption w-28 shrink-0 text-text-secondary">
                {t("settingsDuck.amount")}
              </span>
              <Slider
                value={duck.pct / 100}
                step={0.05}
                label={t("settingsDuck.amount")}
                valueText={t("settingsDuck.pct", { v: duck.pct })}
                onCommit={(v) => save({ ...duck, pct: Math.round(v * 20) * 5 })}
                className="flex-1"
              />
              <span className="t-caption w-14 shrink-0 text-right text-text-tertiary">
                {t("settingsDuck.pct", { v: duck.pct })}
              </span>
            </div>
            <fieldset
              className="m-0 flex flex-wrap gap-2 border-0 p-0"
              aria-label={t("settingsDuck.sensitivity")}
            >
              {DUCK_SENS.map((s) => (
                <Chip
                  key={s}
                  active={duck.sensitivity === s}
                  onClick={() => save({ ...duck, sensitivity: s })}
                >
                  {t(`settingsDuck.sens.${s}`)}
                </Chip>
              ))}
            </fieldset>
            <div className="flex flex-col gap-2">
              <div className="t-body text-text-primary">{t("settingsDuck.ignore")}</div>
              {duck.ignore.map((exe) => (
                <div key={exe} className="flex items-center gap-2">
                  <span className="t-body min-w-0 flex-1 truncate text-text-secondary">{exe}</span>
                  <Button
                    variant="ghost"
                    onClick={() => save({ ...duck, ignore: duck.ignore.filter((x) => x !== exe) })}
                  >
                    {t("settingsDuck.remove")}
                  </Button>
                </div>
              ))}
              <form
                className="flex items-center gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  add();
                }}
              >
                <input
                  value={adding}
                  onChange={(e) => setAdding(e.target.value)}
                  placeholder={t("settingsDuck.addPlaceholder")}
                  aria-label={t("settingsDuck.addPlaceholder")}
                  className="t-body h-(--button-height) min-w-0 flex-1 rounded-(--radius-md) border border-border bg-bg-layer px-3 text-text-primary"
                />
                <Button variant="secondary" type="submit">
                  {t("settingsDuck.add")}
                </Button>
              </form>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

const MIX_MODES = ["gapless", "crossfade", "smart"] as const;
const MIX_INTENSITY = ["soft", "medium", "dj"] as const;
type Mix = {
  mode: (typeof MIX_MODES)[number];
  intensity: (typeof MIX_INTENSITY)[number];
  crossfade: number;
  skipAlbums: boolean;
};

/**
 * «Переходы между треками» (T4.10, FR-PLAY-010, `04` §4.1 — Apple «Crossfade», Spotify
 * «Automix», Rockbox): «Без переходов» (gapless), «Кроссфейд» 1–12 с, «Умный автомикс» with its
 * intensity; albums played in order are never mixed unless the switch is off.
 */
function TransitionsSection() {
  const { t } = useTranslation("shell");
  const [mix, setMix] = useState<Mix>({
    mode: "smart",
    intensity: "medium",
    crossfade: 6,
    skipAlbums: true,
  });
  useEffect(() => {
    getSettings().then((s) => {
      const a = ((s.playback ?? {}) as { automix?: Record<string, unknown> }).automix;
      setMix({
        mode: MIX_MODES.includes(a?.mode as Mix["mode"]) ? (a?.mode as Mix["mode"]) : "smart",
        intensity: MIX_INTENSITY.includes(a?.intensity as Mix["intensity"])
          ? (a?.intensity as Mix["intensity"])
          : "medium",
        crossfade: typeof a?.crossfade_s === "number" ? a.crossfade_s : 6,
        skipAlbums: a?.skip_albums !== false,
      });
    });
  }, []);
  const save = (next: Mix) => {
    const prev = mix;
    setMix(next);
    patchSettings({
      playback: {
        automix: {
          mode: next.mode,
          intensity: next.intensity,
          crossfade_s: next.crossfade,
          skip_albums: next.skipAlbums,
        },
      },
    }).catch(() => setMix(prev));
  };
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsMix.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <fieldset
          className="m-0 flex flex-wrap gap-2 border-0 p-0"
          aria-label={t("settingsMix.title")}
        >
          {MIX_MODES.map((m) => (
            <Chip key={m} active={mix.mode === m} onClick={() => save({ ...mix, mode: m })}>
              {t(`settingsMix.mode.${m}`)}
            </Chip>
          ))}
        </fieldset>
        <div className="t-caption text-text-secondary">{t(`settingsMix.hint.${mix.mode}`)}</div>
        {mix.mode === "smart" && (
          <fieldset
            className="m-0 flex flex-wrap gap-2 border-0 p-0"
            aria-label={t("settingsMix.intensity")}
          >
            {MIX_INTENSITY.map((i) => (
              <Chip
                key={i}
                active={mix.intensity === i}
                onClick={() => save({ ...mix, intensity: i })}
              >
                {t(`settingsMix.intensityName.${i}`)}
              </Chip>
            ))}
          </fieldset>
        )}
        {mix.mode === "crossfade" && (
          <div className="flex items-center gap-4">
            <span className="t-caption w-28 shrink-0 text-text-secondary">
              {t("settingsMix.length")}
            </span>
            <Slider
              value={(mix.crossfade - 1) / 11}
              step={1 / 11}
              label={t("settingsMix.length")}
              valueText={t("settingsMix.seconds", { count: mix.crossfade })}
              onCommit={(v) => save({ ...mix, crossfade: Math.round(1 + v * 11) })}
              className="flex-1"
            />
            <span className="t-caption w-14 shrink-0 text-right text-text-tertiary">
              {t("settingsMix.seconds", { count: mix.crossfade })}
            </span>
          </div>
        )}
        {mix.mode !== "gapless" && (
          <div className="flex items-center gap-4">
            <div className="min-w-0 flex-1">
              <div className="t-body text-text-primary">{t("settingsMix.skipAlbums")}</div>
              <div className="t-caption text-text-secondary">{t("settingsMix.skipAlbumsHint")}</div>
            </div>
            <Switch
              checked={mix.skipAlbums}
              label={t("settingsMix.skipAlbums")}
              onChange={(v) => save({ ...mix, skipAlbums: v })}
            />
          </div>
        )}
      </div>
    </div>
  );
}

/** «Визуализация» (FR-VIZ-006/010): blend with the accent, the minimalist wave screen. */
function VizSection() {
  const { t } = useTranslation("shell");
  const [viz, setViz] = useState({ follow_theme_accent: true, minimal: false });
  useEffect(() => {
    getSettings().then((s) => {
      const v = s.viz as { follow_theme_accent?: boolean; minimal?: boolean } | undefined;
      setViz({
        follow_theme_accent: v?.follow_theme_accent ?? true,
        minimal: v?.minimal ?? false,
      });
    });
  }, []);
  const toggle = (key: "follow_theme_accent" | "minimal", v: boolean) => {
    setViz((cur) => ({ ...cur, [key]: v }));
    patchSettings({ viz: { [key]: v } }).catch(() => setViz((cur) => ({ ...cur, [key]: !v })));
  };
  const rows = [
    {
      key: "follow_theme_accent",
      label: "settingsViz.followAccent",
      hint: "settingsViz.followAccentHint",
    },
    { key: "minimal", label: "settingsViz.minimal", hint: "settingsViz.minimalHint" },
  ] as const;
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsViz.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        {rows.map((r) => (
          <div key={r.key} className="flex items-center gap-4">
            <div className="min-w-0 flex-1">
              <div className="t-body text-text-primary">{t(r.label)}</div>
              <div className="t-caption text-text-secondary">{t(r.hint)}</div>
            </div>
            <Switch checked={viz[r.key]} label={t(r.label)} onChange={(v) => toggle(r.key, v)} />
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * «Место и погода» (T5.5, FR-CTX-001/002/004): the city by name (Open-Meteo geocoder) or
 * «Определить автоматически» (the Windows location service, only on this press, D-54); the
 * weather line. Without a city — the daypart by the clock and no weather (works offline).
 */
function PlaceSection() {
  const { t } = useTranslation("shell");
  const weather = useWeather();
  const [city, setCity] = useState<City | null>(null);
  const [q, setQ] = useState("");
  const [hits, setHits] = useState<City[]>([]);
  const [problem, setProblem] = useState<string | null>(null);
  const [, tick] = useState(0);
  useEffect(() => {
    getSettings().then((s) => {
      const c = (s.context as { city?: City | null } | undefined)?.city ?? null;
      setCity(c);
    });
    const id = window.setInterval(() => tick((n) => n + 1), 60_000);
    return () => window.clearInterval(id);
  }, []);
  useEffect(() => {
    const needle = q.trim();
    if (needle.length < 2) {
      setHits([]);
      return;
    }
    const id = window.setTimeout(() => {
      citySearch(needle)
        .then((r) => {
          setHits(r);
          setProblem(r.length === 0 ? "none" : null);
        })
        .catch(() => setProblem("offline"));
    }, 300);
    return () => window.clearTimeout(id);
  }, [q]);
  const choose = (c: City | null) => {
    const prev = city;
    setCity(c);
    setQ("");
    setHits([]);
    setProblem(null);
    patchSettings({ context: { city: c } }).catch(() => setCity(prev));
  };
  const minutes = weather.fetchedAt ? Math.round((Date.now() - weather.fetchedAt) / 60_000) : 0;
  const cityName = city ? city.name || t("settingsPlace.here") : t("settingsPlace.none");
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsPlace.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <div className="flex flex-wrap items-center gap-3">
          <div className="min-w-0 flex-1">
            <div className="t-body text-text-primary" data-testid="place-city">
              {cityName}
              {city?.admin ? `, ${city.admin}` : ""}
              {city?.country ? `, ${city.country}` : ""}
            </div>
            <div className="t-caption text-text-secondary">
              {city ? t("settingsPlace.hintSet") : t("settingsPlace.hintNone")}
            </div>
          </div>
          {city && (
            <Button variant="ghost" onClick={() => choose(null)}>
              {t("settingsPlace.clear")}
            </Button>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-3">
          <input
            type="search"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={t("settingsPlace.search")}
            aria-label={t("settingsPlace.search")}
            className="t-body h-(--button-height) min-w-0 flex-1 rounded-full border border-border bg-bg-layer px-4 text-text-primary"
          />
          <Button
            variant="secondary"
            onClick={() =>
              void cityLocate()
                .then((c) => choose(c))
                .catch(() => setProblem("denied"))
            }
          >
            {t("settingsPlace.auto")}
          </Button>
        </div>
        {hits.length > 0 && (
          <ul className="m-0 flex list-none flex-col p-0" aria-label={t("settingsPlace.search")}>
            {hits.map((h) => (
              <li key={`${h.lat},${h.lon}`}>
                <button
                  type="button"
                  onClick={() => choose(h)}
                  className="t-body w-full rounded-(--radius-md) px-3 py-2 text-left text-text-primary hover:bg-bg-hover"
                >
                  {h.name}
                  <span className="t-caption text-text-secondary">
                    {[h.admin, h.country].filter(Boolean).join(", ") &&
                      ` · ${[h.admin, h.country].filter(Boolean).join(", ")}`}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
        {problem && (
          <div className="t-caption text-text-secondary">
            {t(`settingsPlace.problem.${problem}`)}
          </div>
        )}
        {city && (
          <div className="t-caption text-text-secondary" data-testid="place-weather">
            {weather.class
              ? t("settingsPlace.weather", {
                  what: t(`settingsPlace.class.${weather.class}`),
                  temp: Math.round(weather.tempC ?? 0),
                  minutes,
                })
              : t("settingsPlace.noWeather")}
          </div>
        )}
        <div className="t-caption text-text-tertiary">{t("settingsPlace.credit")}</div>
      </div>
    </div>
  );
}

const FOCUS_VOCALS = ["none", "prefer_none", "any"] as const;

/**
 * «Фокус» (T4.24, FR-FOCUS-001/003/004/006): «свой» cycle (work, break, long break, cycles
 * before it), «Слова» (Q-5, default «Предпочтительно без слов»), the allowed overrun at the
 * boundary, and normalization for the session.
 */
function FocusSection() {
  const { t } = useTranslation("shell");
  const [f, setF] = useState({
    work_min: 25,
    break_min: 5,
    long_break_min: 15,
    cycles_before_long: 4,
    vocals: "prefer_none" as (typeof FOCUS_VOCALS)[number],
    overrun_s: 90,
    auto_normalize: true,
  });
  useEffect(() => {
    getSettings().then((d) => {
      const v = (d.focus ?? {}) as Partial<typeof f>;
      setF((cur) => ({ ...cur, ...v }));
    });
  }, []);
  const save = (next: typeof f) => {
    const prev = f;
    setF(next);
    patchSettings({ focus: next }).catch(() => setF(prev));
  };
  const rows = [
    { key: "work_min", min: 5, max: 180, label: "settingsFocus.work" },
    { key: "break_min", min: 1, max: 60, label: "settingsFocus.break" },
    { key: "long_break_min", min: 1, max: 60, label: "settingsFocus.longBreak" },
  ] as const;
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsFocus.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        {rows.map((r) => (
          <div key={r.key} className="flex items-center gap-4">
            <span className="t-caption w-28 shrink-0 text-text-secondary">{t(r.label)}</span>
            <Slider
              value={(f[r.key] - r.min) / (r.max - r.min)}
              step={1 / (r.max - r.min)}
              label={t(r.label)}
              valueText={t("settingsSleep.fadeValue", { count: f[r.key] })}
              onCommit={(v) => save({ ...f, [r.key]: Math.round(r.min + v * (r.max - r.min)) })}
              className="flex-1"
            />
          </div>
        ))}
        <div className="flex items-center gap-4">
          <span className="t-caption w-28 shrink-0 text-text-secondary">
            {t("settingsFocus.overrun")}
          </span>
          <Slider
            value={f.overrun_s / 300}
            step={10 / 300}
            label={t("settingsFocus.overrun")}
            valueText={t("settingsFocus.seconds", { count: f.overrun_s })}
            onCommit={(v) => save({ ...f, overrun_s: Math.round((v * 300) / 10) * 10 })}
            className="flex-1"
          />
        </div>
        <span className="t-body text-text-primary">{t("settingsFocus.vocals")}</span>
        <fieldset
          className="m-0 flex flex-wrap gap-2 border-0 p-0"
          aria-label={t("settingsFocus.vocals")}
        >
          {FOCUS_VOCALS.map((v) => (
            <Chip key={v} active={f.vocals === v} onClick={() => save({ ...f, vocals: v })}>
              {t(`settingsFocus.vocalsName.${v}`)}
            </Chip>
          ))}
        </fieldset>
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <div className="t-body text-text-primary">{t("settingsFocus.normalize")}</div>
            <div className="t-caption text-text-secondary">{t("settingsFocus.normalizeHint")}</div>
          </div>
          <Switch
            checked={f.auto_normalize}
            label={t("settingsFocus.normalize")}
            onChange={(v) => save({ ...f, auto_normalize: v })}
          />
        </div>
      </div>
    </div>
  );
}

const SLEEP_AFTER = ["nothing", "screen_off", "sleep", "hibernate"] as const;

/**
 * «Сон-таймер» (T4.21–T4.23, FR-SLEEP-004/007, `14` §14): how long the fade lasts, «тёплый
 * звук», and what happens after — nothing by default; «Выключить экран», «Сон», «Гибернация»
 * only by this choice (hibernate only where Windows has it, Q-17; on Modern Standby «сон» turns
 * the screen off and lets the PC sleep by itself, Q-4).
 */
function SleepSection() {
  const { t } = useTranslation("shell");
  const [s, setS] = useState<{
    fade_min: number;
    warm: boolean;
    after: (typeof SLEEP_AFTER)[number];
  }>({ fade_min: 10, warm: false, after: "nothing" });
  const [caps, setCaps] = useState({ hibernate: false, modernStandby: false });
  useEffect(() => {
    getSettings().then((d) => {
      const v = d.sleep as
        | { fade_min?: number; warm?: boolean; after?: (typeof SLEEP_AFTER)[number] }
        | undefined;
      setS({ fade_min: v?.fade_min ?? 10, warm: v?.warm ?? false, after: v?.after ?? "nothing" });
    });
    sleepCaps()
      .then(setCaps)
      .catch(() => {});
  }, []);
  const save = (next: typeof s) => {
    const prev = s;
    setS(next);
    patchSettings({ sleep: next }).catch(() => setS(prev));
  };
  const choices = SLEEP_AFTER.filter((a) => a !== "hibernate" || caps.hibernate);
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsSleep.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <div className="flex items-center gap-4">
          <span className="t-caption w-28 shrink-0 text-text-secondary">
            {t("settingsSleep.fade")}
          </span>
          <Slider
            value={(s.fade_min - 1) / 29}
            step={1 / 29}
            label={t("settingsSleep.fade")}
            valueText={t("settingsSleep.fadeValue", { count: s.fade_min })}
            onCommit={(v) => save({ ...s, fade_min: Math.round(1 + v * 29) })}
            className="flex-1"
          />
        </div>
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <div className="t-body text-text-primary">{t("settingsSleep.warm")}</div>
            <div className="t-caption text-text-secondary">{t("settingsSleep.warmHint")}</div>
          </div>
          <Switch
            checked={s.warm}
            label={t("settingsSleep.warm")}
            onChange={(v) => save({ ...s, warm: v })}
          />
        </div>
        <span className="t-body text-text-primary">{t("settingsSleep.after")}</span>
        <fieldset
          className="m-0 flex flex-wrap gap-2 border-0 p-0"
          aria-label={t("settingsSleep.after")}
        >
          {choices.map((a) => (
            <Chip key={a} active={s.after === a} onClick={() => save({ ...s, after: a })}>
              {t(`settingsSleep.afterName.${a}`)}
            </Chip>
          ))}
        </fieldset>
        {s.after === "sleep" && caps.modernStandby && (
          <div className="t-caption text-text-secondary">{t("settingsSleep.modernStandby")}</div>
        )}
      </div>
    </div>
  );
}

const FX_MODES = ["weather", "season", "manual", "off"] as const;

/**
 * «Живые эффекты» (T4.20, FR-FX-001/007, supplement §31): the mode (by the real weather — the
 * default; by the season; by hand; off), the manual pick and the still version for weak PCs.
 */
function FxSection() {
  const { t } = useTranslation("shell");
  const [fx, setFx] = useState<{
    mode: (typeof FX_MODES)[number];
    manual: FxType;
    static_when_weak: boolean;
  }>({ mode: "weather", manual: "snow", static_when_weak: false });
  useEffect(() => {
    getSettings().then((s) => {
      const f = s.fx as
        | { mode?: (typeof FX_MODES)[number]; manual?: string | null; static_when_weak?: boolean }
        | undefined;
      setFx({
        mode: f?.mode ?? "weather",
        manual: isFxType(f?.manual) ? f.manual : "snow",
        static_when_weak: f?.static_when_weak ?? false,
      });
    });
  }, []);
  const save = (next: typeof fx) => {
    const prev = fx;
    setFx(next);
    patchSettings({ fx: next }).catch(() => setFx(prev));
  };
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsFx.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <fieldset
          className="m-0 flex flex-wrap gap-2 border-0 p-0"
          aria-label={t("settingsFx.title")}
        >
          {FX_MODES.map((m) => (
            <Chip key={m} active={fx.mode === m} onClick={() => save({ ...fx, mode: m })}>
              {t(`settingsFx.mode.${m}`)}
            </Chip>
          ))}
        </fieldset>
        <div className="t-caption text-text-secondary">{t(`settingsFx.hint.${fx.mode}`)}</div>
        {fx.mode === "manual" && (
          <fieldset
            className="m-0 flex flex-wrap gap-2 border-0 p-0"
            aria-label={t("settingsFx.pick")}
          >
            {FX_TYPES.map((k) => (
              <Chip key={k} active={fx.manual === k} onClick={() => save({ ...fx, manual: k })}>
                {t(`settingsFx.type.${k}`)}
              </Chip>
            ))}
          </fieldset>
        )}
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <div className="t-body text-text-primary">{t("settingsFx.static")}</div>
            <div className="t-caption text-text-secondary">{t("settingsFx.staticHint")}</div>
          </div>
          <Switch
            checked={fx.static_when_weak}
            label={t("settingsFx.static")}
            onChange={(v) => save({ ...fx, static_when_weak: v })}
          />
        </div>
      </div>
    </div>
  );
}

/** «Моя волна» (FR-LEARN, docs/14 §8.4): learning on/off and «Сбросить обучение». */
function WaveLearningSection() {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [on, setOn] = useState(true);
  const [asking, setAsking] = useState(false);
  useEffect(() => {
    getSettings().then((s) => {
      const w = s.wave as { learning?: boolean } | undefined;
      setOn(w?.learning ?? true);
    });
  }, []);
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsWave.title")}</h2>
      <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <div className="flex items-center gap-4">
          <div className="min-w-0 flex-1">
            <div className="t-body text-text-primary">{t("settingsWave.learning")}</div>
            <div className="t-caption text-text-secondary">{t("settingsWave.learningHint")}</div>
          </div>
          <Switch
            checked={on}
            label={t("settingsWave.learning")}
            onChange={(v) => {
              setOn(v);
              patchSettings({ wave: { learning: v } }).catch(() => setOn(!v));
            }}
          />
        </div>
        <AnalysisLine />
        <Button variant="ghost" className="self-start text-danger" onClick={() => setAsking(true)}>
          {t("settingsWave.reset")}
        </Button>
      </div>
      {asking && (
        <Dialog
          title={t("settingsWave.reset")}
          closeLabel={tc("errors.close")}
          onClose={() => setAsking(false)}
        >
          <p className="t-body text-text-primary">{t("settingsWave.resetQuestion")}</p>
          <div className="flex flex-wrap gap-3">
            <Button
              variant="secondary"
              className="text-danger"
              onClick={() => {
                setAsking(false);
                resetLearning()
                  .then(() => notify(t("settingsWave.resetDone")))
                  .catch(() => notify(t("wave.failed")));
              }}
            >
              {t("settingsWave.reset")}
            </Button>
            <Button variant="ghost" onClick={() => setAsking(false)}>
              {t("trackCard.cancel")}
            </Button>
          </div>
        </Dialog>
      )}
    </div>
  );
}

/** «Анализ звука: N из M» — the background analysis (T3.6/T3.10), refreshed every 5 s. */
function AnalysisLine() {
  const { t } = useTranslation("shell");
  const [p, setP] = useState<{ done: number; total: number; paused: boolean } | null>(null);
  useEffect(() => {
    let alive = true;
    const load = () =>
      analysisProgress()
        .then((v) => {
          if (alive) setP(v);
        })
        .catch(() => {});
    void load();
    const id = window.setInterval(load, 5000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);
  if (!p || p.total === 0) return null;
  return (
    <p className="t-caption text-text-secondary">
      {p.done >= p.total
        ? t("settingsWave.analysisDone", { count: p.total })
        : t(p.paused ? "settingsWave.analysisPaused" : "settingsWave.analysis", {
            done: p.done,
            total: p.total,
          })}
    </p>
  );
}

/** «Дубли: N — Показать» (T3.5): pairs by sound waiting for a decision. */
function DuplicatesLine({ onShow }: { onShow: () => void }) {
  const { t } = useTranslation("shell");
  const [n, setN] = useState<number | null>(null);
  useEffect(() => {
    duplicatesCount()
      .then(setN)
      .catch(() => {});
  }, []);
  if (n === null) return null;
  return (
    <div className="flex flex-wrap items-center gap-3">
      <span className="t-body text-text-primary">
        {n > 0 ? t("dups.pending", { count: n }) : t("dups.none")}
      </span>
      <button type="button" onClick={onShow} className="t-body-strong w-fit text-accent-text">
        {t("dups.show")}
      </button>
    </div>
  );
}

/**
 * «Навести порядок» (T3.19, FR-FS-005): files added in Explorer are taken where they are; the
 * switch moves new ones by the rules at once (otherwise a toast offers it); the button puts the
 * whole library in order.
 */
function OrganizeLine() {
  const { t } = useTranslation("shell");
  const [auto, setAuto] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    getSettings()
      .then((s) => {
        const l = s.library as { organize_external?: string } | undefined;
        setAuto(l?.organize_external === "auto");
      })
      .catch(() => {});
  }, []);
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-4">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="t-body text-text-primary">{t("organize.auto")}</span>
          <span className="t-caption text-text-secondary">{t("organize.autoHint")}</span>
        </div>
        <Switch
          checked={auto}
          label={t("organize.auto")}
          onChange={(on) => {
            setAuto(on);
            patchSettings({ library: { organize_external: on ? "auto" : "suggest" } }).catch(() =>
              setAuto(!on),
            );
          }}
        />
      </div>
      <button
        type="button"
        disabled={busy}
        onClick={() => {
          setBusy(true);
          organizeLibrary(true)
            .then((n) => notify(t("organize.done", { count: n })))
            .catch(() => notify(t("organize.failed")))
            .finally(() => setBusy(false));
        }}
        className="t-body-strong w-fit text-accent-text disabled:opacity-40"
      >
        {t("organize.all")}
      </button>
    </div>
  );
}

/**
 * «Сеть» (T3.1, `03` §11): «Работать полностью офлайн» — no request leaves the computer
 * (recognition, weather, updates); the music, the wave and the search work as before.
 */
function NetworkSection() {
  const { t } = useTranslation("shell");
  const [off, setOff] = useState(false);
  useEffect(() => {
    getSettings()
      .then((s) =>
        setOff(Boolean((s.network as { offline_mode?: boolean } | undefined)?.offline_mode)),
      )
      .catch(() => {});
  }, []);
  return (
    <div className="flex flex-col gap-2">
      <h2 className="t-shelf-title text-primary-text">{t("settingsNet.title")}</h2>
      <div className="flex items-center gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="t-body text-text-primary">{t("settingsNet.offline")}</span>
          <span className="t-caption text-text-secondary">{t("settingsNet.offlineHint")}</span>
        </div>
        <Switch
          checked={off}
          label={t("settingsNet.offline")}
          onChange={(on) => {
            setOff(on);
            patchSettings({ network: { offline_mode: on } }).catch(() => setOff(!on));
          }}
        />
      </div>
    </div>
  );
}
