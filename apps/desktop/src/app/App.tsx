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
import { next as nextTrack, prev as prevTrack, togglePlay } from "@/shared/ipc/playback";
import { createPlaylist } from "@/shared/ipc/playlists";
import {
  getSettings,
  migrateLegacyAppearance,
  onSettingsChanged,
  patchSettings,
  type SettingsDoc,
  type SettingsPatch,
} from "@/shared/ipc/settings";
import { type UpdateStatus, updateStatus } from "@/shared/ipc/updates";
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
import { notify, onNotify } from "@/shared/ui/notify";
import { Toast } from "@/shared/ui/Toast";
import type { Open } from "./library/CollectionScreen";
import type { GoTo } from "./library/TrackList";
import { PlaylistScreen } from "./playlists/PlaylistScreen";
import { nextName, usePlaylists } from "./playlists/usePlaylists";
import { SearchScreen } from "./search/SearchScreen";
import { EmptyScreen } from "./shell/EmptyScreen";
import { PlayerBar } from "./shell/PlayerBar";
import { QueuePanel } from "./shell/QueuePanel";
import { SettingsScreen } from "./shell/SettingsScreen";
import { type Screen, Sidebar } from "./shell/Sidebar";
import { usePerformance } from "./shell/usePerformance";
import { usePlayback } from "./shell/usePlayback";

/** One place in the app: a sidebar section, maybe with an album/artist opened. */
type NavEntry = {
  screen: Screen;
  open: Open | null;
  /** A playlist page; `fresh` — just created, the name starts in edit mode. */
  playlist?: number;
  fresh?: boolean;
};

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
  // Navigation stack (T2.1): sidebar sections and opened albums/artists; «назад» pops.
  const [stack, setStack] = useState<NavEntry[]>([{ screen: "wave", open: null }]);
  const here = stack[stack.length - 1] ?? { screen: "wave" as Screen, open: null };
  const screen = here.screen;
  const push = (e: NavEntry) =>
    setStack((s) => {
      const top = s[s.length - 1];
      if (
        top &&
        top.screen === e.screen &&
        top.open?.id === e.open?.id &&
        top.open?.kind === e.open?.kind &&
        top.playlist === e.playlist
      ) {
        return s;
      }
      return [...s.slice(-49), e];
    });
  const setScreen = (sc: Screen) => push({ screen: sc, open: null });
  const back = () => setStack((s) => (s.length > 1 ? s.slice(0, -1) : s));
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
  const [queueOpen, setQueueOpen] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  useEffect(() => onNotify(setToast), []);
  // «Сиртон обновлён до X» once, on the first start of a new version (D-76).
  const [updated, setUpdated] = useState<UpdateStatus | null>(null);
  useEffect(() => {
    updateStatus().then((u) => {
      if (u?.justUpdated) setUpdated(u);
    });
  }, []);
  const playlists = usePlaylists();
  const openPlaylist = (id: number, fresh = false) =>
    push({ screen: "playlists", open: null, playlist: id, fresh });
  const newPlaylist = () => {
    const name = nextName(playlists, (n) => t("playlists.defaultName", { n }));
    createPlaylist(name)
      .then((id) => openPlaylist(id, true))
      .catch(() => notify(t("playlists.failed")));
  };
  const goTo: GoTo = (kind, id, title) =>
    push({ screen: kind, open: { kind, id, title, subtitle: "", cover: null } });

  // DESIGN.md §10 keys (Space lives in usePlayback): Alt+← / mouse «back» — назад,
  // Ctrl+→/← — next/previous track, Ctrl+, — settings.
  useEffect(() => {
    const typing = (t: EventTarget | null) =>
      t instanceof HTMLElement &&
      (t.isContentEditable || ["INPUT", "TEXTAREA"].includes(t.tagName));
    const key = (e: KeyboardEvent) => {
      if (typing(e.target)) return;
      if ((e.altKey && e.key === "ArrowLeft") || e.key === "BrowserBack") {
        e.preventDefault();
        back();
      } else if (e.ctrlKey && !e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        nextTrack();
      } else if (e.ctrlKey && !e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        prevTrack();
      } else if (e.ctrlKey && !e.altKey && (e.key === "f" || e.key === "F" || e.code === "KeyF")) {
        // DESIGN.md §10: Ctrl+F — поиск (instead of the WebView find bar).
        e.preventDefault();
        setScreen("search");
      } else if (e.ctrlKey && e.key === ",") {
        e.preventDefault();
        setScreen("settings");
      }
    };
    const mouse = (e: MouseEvent) => {
      if (e.button === 3) {
        e.preventDefault();
        back();
      }
    };
    window.addEventListener("keydown", key);
    window.addEventListener("mouseup", mouse);
    return () => {
      window.removeEventListener("keydown", key);
      window.removeEventListener("mouseup", mouse);
    };
  });
  const [narrow, setNarrow] = useState(() => window.matchMedia(NARROW).matches);
  usePerformance();

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
          badges={{ unknown: lib.status.unknown }}
          onToggle={() => setManualCollapsed((c) => !c)}
          progress={lib.progress}
          onCancelImport={cancelImport}
          offline={lib.status.offline}
          playlists={playlists}
          currentPlaylist={screen === "playlists" ? (here.playlist ?? null) : null}
          playingPlaylist={null}
          onOpenPlaylist={(id) => openPlaylist(id)}
          onNewPlaylist={lib.status.tracks > 0 ? newPlaylist : undefined}
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
          ) : screen === "search" && hasMusic ? (
            <SearchScreen
              playingId={snap.trackId}
              version={lib.version}
              onGo={goTo}
              onOpenPlaylist={(id) => openPlaylist(id)}
            />
          ) : screen === "playlists" && here.playlist != null ? (
            <PlaylistScreen
              key={here.playlist}
              id={here.playlist}
              version={lib.version}
              playingId={snap.trackId}
              editName={here.fresh ?? false}
              onGo={goTo}
              onDeleted={() =>
                setStack((st) => {
                  const left = st.filter((e) => e.playlist !== here.playlist);
                  return left.length > 0 ? left : [{ screen: "wave", open: null }];
                })
              }
            />
          ) : hasMusic && screen === "unknown" && lib.status.unknown > 0 ? (
            <UnknownScreen version={lib.version} playingId={snap.trackId} onGo={goTo} />
          ) : hasMusic && (screen === "albums" || screen === "artists") ? (
            <CollectionScreen
              key={screen}
              kind={screen}
              status={lib.status}
              version={lib.version}
              playingId={snap.trackId}
              open={here.open}
              onOpen={(o) => push({ screen, open: o })}
              onBack={back}
            />
          ) : (
            <EmptyScreen
              screen={screen}
              onAdd={["wave", "albums", "artists"].includes(screen) ? addMusic : undefined}
            />
          )}
        </main>
        {queueOpen && (
          <div className="flex shrink-0 bg-bg-layer p-3 pl-0">
            <QueuePanel
              onClose={() => setQueueOpen(false)}
              trackKey={`${snap.trackId}-${snap.title}`}
            />
          </div>
        )}
      </div>
      <div className="t-caption bg-bg-base px-4 pt-1 text-center text-text-tertiary">
        {t("proto")}
      </div>
      <PlayerBar
        snap={snap}
        receivedAt={receivedAt}
        queueOpen={queueOpen}
        onQueue={() => setQueueOpen((o) => !o)}
      />
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
      {updated?.justUpdated && !error && !report && (
        <Toast
          tone="info"
          heading={t("updates.updatedTitle", { version: updated.justUpdated })}
          message={firstLine(updated.notes) ?? t("updates.updatedText")}
          closeLabel={tc("errors.close")}
          timeoutMs={8000}
          onClose={() => setUpdated(null)}
        />
      )}
      {toast && !error && !report && (
        <Toast
          tone="info"
          heading={toast}
          message=""
          closeLabel={tc("errors.close")}
          timeoutMs={3000}
          onClose={() => setToast(null)}
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

/** The first line of release notes, for a one-line toast. */
function firstLine(text: string | null): string | null {
  const line = text?.split(/\r?\n/).find((l) => l.trim() !== "");
  return line ? line.replace(/^[#*\s-]+/, "").replace(/\*\*/g, "") : null;
}
