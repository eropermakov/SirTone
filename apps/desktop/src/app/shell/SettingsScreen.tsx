import { Check, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CollectionSummary } from "@/app/library/CollectionScreen";
import type { LibraryStatus } from "@/shared/ipc/library";
import { getSettings, patchSettings } from "@/shared/ipc/settings";
import {
  autostartAvailable,
  type HotkeyAction,
  type HotkeyStatus,
  hotkeysStatus,
  setAutostart,
} from "@/shared/ipc/window";
import { accelFromEvent } from "@/shared/lib/hotkeys";
import { cn } from "@/shared/lib/utils";
import { ACCENT_SLUGS, type AccentSource, isPalette, type ThemePref } from "@/shared/theme/theme";
import { Button } from "@/shared/ui/Button";
import { Switch } from "@/shared/ui/Switch";

type Props = {
  theme: ThemePref;
  accent: AccentSource;
  onTheme: (t: ThemePref) => void;
  onAccent: (a: AccentSource) => void;
  /** «Хранилище» (FR-DATA-004, FR-ONB-003): library folder and counters. */
  library?: LibraryStatus;
  onChooseLibrary?: () => void;
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
}: Props) {
  const { t } = useTranslation("shell");
  const customHex = accent.kind === "custom" ? accent.hex : "#3b82f6";

  return (
    <section className="flex max-w-3xl flex-col gap-12">
      <h1 className="t-page-title text-primary-text">{t("settings.title")}</h1>

      <div className="flex flex-col gap-4">
        <h2 className="t-shelf-title text-primary-text">{t("settings.appearance")}</h2>

        <div className="flex flex-col gap-3 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
          <span className="t-card-title text-text-primary">{t("settings.theme")}</span>
          <fieldset
            className="m-0 border-0 p-0 flex flex-wrap gap-2"
            aria-label={t("settings.theme")}
          >
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
        </div>

        <div className="flex flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6">
          <span className="t-card-title text-text-primary">{t("settings.color")}</span>
          {isPalette(theme) && (
            <span className="t-caption text-text-secondary">{t("settings.accentByTheme")}</span>
          )}
          <fieldset
            disabled={isPalette(theme)}
            className="m-0 border-0 p-0 flex flex-wrap gap-3 disabled:opacity-40"
            aria-label={t("settings.color")}
          >
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
                    <Check
                      size={16}
                      strokeWidth={2}
                      style={{ color: `var(--accent-${slug}-on)` }}
                    />
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

      <StartupSection />

      <HotkeysSection />

      <div className="flex flex-col gap-2">
        <h2 className="t-shelf-title text-primary-text">{t("settings.about")}</h2>
        <p className="t-body text-text-secondary">
          {t("settings.version", { version: __APP_VERSION__ })}
        </p>
        <p className="t-caption text-text-tertiary">{t("updates.auto")}</p>
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
