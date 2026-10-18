import { Check, Gauge, Plus } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CollectionSummary } from "@/app/library/CollectionScreen";
import { mb } from "@/app/library/TrackCard";
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
import { getSettings, patchSettings } from "@/shared/ipc/settings";
import { clearOldLogs, type StorageInfo, storageInfo } from "@/shared/ipc/storage";
import {
  autostartAvailable,
  type HotkeyAction,
  type HotkeyStatus,
  hotkeysStatus,
  setAutostart,
} from "@/shared/ipc/window";
import { accelFromEvent } from "@/shared/lib/hotkeys";
import { cn } from "@/shared/lib/utils";
import { benchmark, score } from "@/shared/theme/autodetect";
import { applyPerformance, profile } from "@/shared/theme/performance";
import { ACCENT_SLUGS, type AccentSource, isPalette, type ThemePref } from "@/shared/theme/theme";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Dialog } from "@/shared/ui/Dialog";
import { notify } from "@/shared/ui/notify";
import { Switch } from "@/shared/ui/Switch";

type Props = {
  theme: ThemePref;
  accent: AccentSource;
  onTheme: (t: ThemePref) => void;
  onAccent: (a: AccentSource) => void;
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
          <AccentSwatches theme={theme} accent={accent} onAccent={onAccent} />
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

      <PerformanceSection />

      <StartupSection />

      <HotkeysSection />

      <WaveLearningSection />

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
  useEffect(() => {
    getSettings().then((doc) => {
      const p = (doc.performance ?? {}) as { level?: number; overrides?: Record<string, unknown> };
      setLevel(p.level ?? 3);
      setOverrides(p.overrides ?? {});
    });
  }, []);
  const levels = t("settingsPerf.level", { returnObjects: true }) as unknown as string[];
  const eff = profile(level, overrides);
  // Choosing a preset clears the point switches: the preset is what you asked for.
  const choose = (l: number) => {
    setLevel(l);
    setOverrides({});
    applyPerformance(profile(l, {}));
    patchSettings({ performance: { level: l, overrides: {} } }).catch(() => {});
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
  onAccent,
}: {
  theme: ThemePref;
  accent: AccentSource;
  onAccent: (a: AccentSource) => void;
}) {
  const { t } = useTranslation("shell");
  const customHex = accent.kind === "custom" ? accent.hex : "#3b82f6";
  return (
    <fieldset
      disabled={isPalette(theme)}
      className="m-0 border-0 p-0 flex flex-wrap gap-3 disabled:opacity-40"
      aria-label={t("settings.color")}
    >
      <button
        type="button"
        aria-pressed={accent.kind === "auto"}
        aria-label={t("accent.auto")}
        title={t("accent.autoHint")}
        onClick={() => onAccent({ kind: "auto" })}
        className={cn(
          "relative flex size-8 items-center justify-center rounded-full",
          accent.kind === "auto" &&
            "ring-2 ring-text-primary ring-offset-[3px] ring-offset-bg-raised",
        )}
        style={{
          // DESIGN.md §9 AccentPicker: «Авто» — four sectors, first in the row.
          background:
            "conic-gradient(var(--accent-blue-fill) 0 25%, var(--accent-yellow-fill) 0 50%, var(--accent-red-fill) 0 75%, var(--accent-sirkov-fill) 0)",
        }}
      />
      {ACCENT_SLUGS.map((slug) => {
        const selected = accent.kind === "preset" && accent.slug === slug;
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
          accent.kind === "custom" &&
            "ring-2 ring-text-primary ring-offset-[3px] ring-offset-bg-raised",
        )}
        style={accent.kind === "custom" ? { background: customHex } : undefined}
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

/** «Тема по сезону» (FR-ONB-001 step 2, FR-UI-003): stored now, the seasonal themes come with
 * Stage 4. */
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
