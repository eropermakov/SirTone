import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { togglePlay } from "@/shared/ipc/playback";
import {
  getSettings,
  migrateLegacyAppearance,
  onSettingsChanged,
  patchSettings,
  type SettingsDoc,
  type SettingsPatch,
} from "@/shared/ipc/settings";
import { setWindowTheme } from "@/shared/ipc/window";
import {
  ACCENT_SLUGS,
  type AccentSlug,
  type AccentSource,
  applyAppearance,
  onSystemThemeChange,
  resolveTheme,
  type ThemePref,
} from "@/shared/theme/theme";
import { Toast } from "@/shared/ui/Toast";
import { EmptyScreen } from "./shell/EmptyScreen";
import { PlayerBar } from "./shell/PlayerBar";
import { SettingsScreen } from "./shell/SettingsScreen";
import { type Screen, Sidebar } from "./shell/Sidebar";
import { usePlayback } from "./shell/usePlayback";

/** Shell (prototype slice of T2.1/T2.2): three zones from DESIGN.md §7. Appearance lives in Rust settings (T0.6). */
type Appearance = { theme: ThemePref; accent: AccentSource };
const DEFAULT: Appearance = { theme: "system", accent: { kind: "preset", slug: "sirkov" } };

function fromDoc(doc: SettingsDoc): Appearance {
  const a = doc.appearance.accent;
  const accent: AccentSource =
    a.source === "custom" && a.custom
      ? { kind: "custom", hex: a.custom }
      : {
          kind: "preset",
          slug: (ACCENT_SLUGS as readonly string[]).includes(a.preset)
            ? (a.preset as AccentSlug)
            : "sirkov",
        };
  return { theme: doc.appearance.theme, accent };
}

function toPatch(a: Appearance): SettingsPatch {
  const accent =
    a.accent.kind === "custom"
      ? { source: "custom", custom: a.accent.hex }
      : { source: "preset", preset: a.accent.slug };
  return { appearance: { theme: a.theme, accent } };
}

const NARROW = "(max-width: 899px)";

export function App() {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const { snap, receivedAt, error, clearError } = usePlayback();
  const [screen, setScreen] = useState<Screen>("wave");
  const [appearance, setAppearanceState] = useState<Appearance>(DEFAULT);
  const [manualCollapsed, setManualCollapsed] = useState(false);
  const [narrow, setNarrow] = useState(() => window.matchMedia(NARROW).matches);

  // Load settings once (and move prototype 0.1 choices over), then follow changes from Rust.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const reload = () => getSettings().then((doc) => setAppearanceState(fromDoc(doc)));
    migrateLegacyAppearance().finally(reload);
    onSettingsChanged(reload).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, []);

  /** Optimistic UI (DESIGN.md §1): apply at once, persist in the background, roll back on error. */
  const setAppearance = (next: (a: Appearance) => Appearance) => {
    const prev = appearance;
    const value = next(prev);
    setAppearanceState(value);
    patchSettings(toPatch(value)).catch(() => setAppearanceState(prev));
  };

  useEffect(() => {
    const apply = () => {
      applyAppearance(appearance.theme, appearance.accent);
      setWindowTheme(resolveTheme(appearance.theme));
    };
    apply();
    if (appearance.theme !== "system") return;
    return onSystemThemeChange(apply);
  }, [appearance]);

  useEffect(() => {
    const m = window.matchMedia(NARROW);
    const on = () => setNarrow(m.matches);
    m.addEventListener("change", on);
    on(); // the window may have resized before we subscribed (hidden at start)
    return () => m.removeEventListener("change", on);
  }, []);

  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-0 flex-1">
        <Sidebar
          current={screen}
          collapsed={narrow || manualCollapsed}
          onNavigate={setScreen}
          onToggle={() => setManualCollapsed((c) => !c)}
        />
        <main className="min-w-0 flex-1 overflow-auto rounded-tl-(--radius-xl) bg-bg-layer p-6">
          {screen === "settings" ? (
            <SettingsScreen
              theme={appearance.theme}
              accent={appearance.accent}
              onTheme={(theme) => setAppearance((a) => ({ ...a, theme }))}
              onAccent={(accent) => setAppearance((a) => ({ ...a, accent }))}
            />
          ) : (
            <EmptyScreen screen={screen} />
          )}
        </main>
      </div>
      <div className="t-caption bg-bg-base px-4 pt-1 text-center text-text-tertiary">
        {t("proto")}
      </div>
      <PlayerBar snap={snap} receivedAt={receivedAt} />
      {error && (
        <Toast
          heading={tc("errors.title")}
          message={[
            tc(`errors.${error.error.code}`, { defaultValue: tc("errors.generic") }),
            error.file,
          ]
            .filter(Boolean)
            .join(" · ")}
          closeLabel={tc("errors.close")}
          action={
            error.error.retryable
              ? {
                  label: tc("errors.retry"),
                  onClick: () => {
                    clearError();
                    togglePlay();
                  },
                }
              : undefined
          }
          onClose={clearError}
        />
      )}
    </div>
  );
}
