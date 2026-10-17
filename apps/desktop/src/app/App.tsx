import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CollectionScreen } from "@/app/library/CollectionScreen";
import { RootPanel } from "@/app/library/RootPanel";
import { UnknownScreen } from "@/app/library/UnknownScreen";
import { useLibrary } from "@/app/library/useLibrary";
import { bootstrap } from "@/shared/ipc/bootstrap";
import {
  cancelImport,
  confirmMissing,
  dismissQuestion,
  importPaths,
  type LibraryStatus,
  pickFolder,
} from "@/shared/ipc/library";
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
  isPalette,
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
  const { snap, receivedAt, error, clearError, hydrate } = usePlayback();
  const [screen, setScreen] = useState<Screen>("wave");
  const [bootLibrary, setBootLibrary] = useState<LibraryStatus | null>(null);
  const lib = useLibrary(bootLibrary);
  // The question about vanished files is answered optimistically: the toast goes at once.
  const confirmGone = () => {
    confirmMissing();
    lib.setStatus((st) => ({ ...st, goneQuestion: null }));
  };
  const dismissGone = () => {
    dismissQuestion();
    lib.setStatus((st) => ({ ...st, goneQuestion: null }));
  };
  // «Добавить папку» pressed before a library folder existed: pick sources right after.
  const [addAfterRoot, setAddAfterRoot] = useState(false);
  const [appearance, setAppearanceState] = useState<Appearance>(DEFAULT);
  const [manualCollapsed, setManualCollapsed] = useState(false);
  const [narrow, setNarrow] = useState(() => window.matchMedia(NARROW).matches);

  // Load settings once (and move prototype 0.1 choices over), then follow changes from Rust.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const reload = () => getSettings().then((doc) => setAppearanceState(fromDoc(doc)));
    migrateLegacyAppearance().finally(() =>
      bootstrap().then((b) => {
        setAppearanceState(fromDoc(b.settings));
        hydrate(b.playback);
        if (b.library) setBootLibrary(b.library);
      }),
    );
    onSettingsChanged(reload).then((u) => {
      unlisten = u;
    });
    return () => unlisten?.();
  }, [hydrate]);

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
      setWindowTheme(
        resolveTheme(appearance.theme),
        isPalette(appearance.theme) ? appearance.theme : null,
      );
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

  const addMusic = async () => {
    if (!lib.status.root) {
      setAddAfterRoot(true);
      lib.setRootNeeded(true);
      return;
    }
    const folder = await pickFolder();
    if (folder) await importPaths([folder]);
  };
  const rootChosen = async (s: LibraryStatus) => {
    lib.setStatus(s);
    lib.setRootNeeded(false);
    if (addAfterRoot) {
      setAddAfterRoot(false);
      const folder = await pickFolder();
      if (folder) await importPaths([folder]);
    }
  };
  const hasMusic = lib.status.tracks > 0;
  const report = lib.report;

  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-0 flex-1">
        <Sidebar
          current={screen}
          collapsed={narrow || manualCollapsed}
          onNavigate={setScreen}
          onToggle={() => setManualCollapsed((c) => !c)}
          progress={lib.progress}
          onCancelImport={cancelImport}
          offline={lib.status.offline}
        />
        <main className="flex min-w-0 flex-1 flex-col overflow-auto rounded-tl-(--radius-xl) bg-bg-layer p-6">
          {lib.rootNeeded && <RootPanel status={lib.status} onDone={rootChosen} />}
          {screen === "settings" ? (
            <SettingsScreen
              theme={appearance.theme}
              accent={appearance.accent}
              onTheme={(theme) => setAppearance((a) => ({ ...a, theme }))}
              onAccent={(accent) => setAppearance((a) => ({ ...a, accent }))}
              library={lib.status}
              onChooseLibrary={() => lib.setRootNeeded(true)}
            />
          ) : hasMusic && screen === "unknown" && lib.status.unknown > 0 ? (
            <UnknownScreen version={lib.version} playingId={snap.trackId} />
          ) : hasMusic && (screen === "albums" || screen === "artists") ? (
            <CollectionScreen
              key={screen}
              kind={screen}
              status={lib.status}
              version={lib.version}
              playingId={snap.trackId}
            />
          ) : (
            <EmptyScreen
              screen={screen}
              onAdd={["wave", "albums", "artists"].includes(screen) ? addMusic : undefined}
            />
          )}
        </main>
      </div>
      <div className="t-caption bg-bg-base px-4 pt-1 text-center text-text-tertiary">
        {t("proto")}
      </div>
      <PlayerBar snap={snap} receivedAt={receivedAt} />
      {lib.status.goneQuestion != null && !error && !report && (
        <Toast
          tone="info"
          heading={t("library.goneTitle")}
          message={t("library.gone", { count: lib.status.goneQuestion })}
          closeLabel={tc("errors.close")}
          action={{ label: t("library.goneConfirm"), onClick: confirmGone }}
          onClose={dismissGone}
        />
      )}
      {report && !error && (
        <Toast
          tone="info"
          heading={t("import.doneTitle")}
          message={[
            t("import.report", {
              added: report.added,
              unknown: report.unknown,
              duplicates: report.duplicates,
              rejected: report.rejected,
            }),
            report.rejectedReasons.length > 0
              ? t("import.reasons", {
                  list: report.rejectedReasons.map((r) => t(`import.reason.${r}`)).join(", "),
                })
              : "",
            report.canEject ? t("import.eject") : "",
          ]
            .filter(Boolean)
            .join(" · ")}
          closeLabel={tc("errors.close")}
          timeoutMs={10000}
          onClose={lib.clearReport}
        />
      )}
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
