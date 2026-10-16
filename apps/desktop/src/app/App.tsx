import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  type AccentSource,
  applyAppearance,
  onSystemThemeChange,
  type ThemePref,
} from "@/shared/theme/theme";
import { EmptyScreen } from "./shell/EmptyScreen";
import { PlayerBar } from "./shell/PlayerBar";
import { SettingsScreen } from "./shell/SettingsScreen";
import { type Screen, Sidebar } from "./shell/Sidebar";

/**
 * Prototype 0.1 shell (slice of T2.1/T2.2): three zones from DESIGN.md §7.
 * Appearance is kept in localStorage only until the Rust settings service (T0.6) exists.
 */
const STORE = "sirtone.proto.appearance";
type Appearance = { theme: ThemePref; accent: AccentSource };
const DEFAULT: Appearance = { theme: "system", accent: { kind: "preset", slug: "sirkov" } };

function load(): Appearance {
  try {
    const raw = localStorage.getItem(STORE);
    return raw ? { ...DEFAULT, ...(JSON.parse(raw) as Appearance) } : DEFAULT;
  } catch {
    return DEFAULT;
  }
}

const NARROW = "(max-width: 899px)";

export function App() {
  const { t } = useTranslation("shell");
  const [screen, setScreen] = useState<Screen>("wave");
  const [appearance, setAppearance] = useState<Appearance>(load);
  const [manualCollapsed, setManualCollapsed] = useState(false);
  const [narrow, setNarrow] = useState(() => window.matchMedia(NARROW).matches);

  useEffect(() => {
    applyAppearance(appearance.theme, appearance.accent);
    try {
      localStorage.setItem(STORE, JSON.stringify(appearance));
    } catch {
      // storage unavailable — appearance still applies for this session
    }
    if (appearance.theme !== "system") return;
    return onSystemThemeChange(() => applyAppearance(appearance.theme, appearance.accent));
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
      <PlayerBar />
    </div>
  );
}
