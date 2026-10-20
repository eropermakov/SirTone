import { expect, type Page, test } from "@playwright/test";

/**
 * T0.11: the UI against a mocked Tauri bridge (mockIPC approach, docs/08 §2). The page runs the
 * real generated `bindings.ts` code: `app_bootstrap` hydration, commands, typed events, error toast.
 */
async function mockTauri(page: Page) {
  await page.addInitScript(() => {
    type Cb = (payload: unknown) => void;
    const callbacks = new Map<number, Cb>();
    const listeners = new Map<string, number[]>();
    let nextId = 1;
    const calls: Array<{ cmd: string; args: unknown }> = [];
    const unknown: string[] = [];
    const settings = {
      appearance: { theme: "dark", accent: { source: "preset", preset: "blue", custom: null } },
      playback: { volume: 0.5 },
      onboarding: { done: true, tips_seen: true },
    };
    const w = window as unknown as Record<string, unknown>;
    w.isTauri = true;
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { windowLabel: "main", label: "main" },
      },
      transformCallback: (cb: Cb) => {
        const id = nextId++;
        callbacks.set(id, cb);
        return id;
      },
      unregisterCallback: (id: number) => callbacks.delete(id),
      convertFileSrc: (p: string) => p,
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        calls.push({ cmd, args });
        switch (cmd) {
          case "plugin:event|listen": {
            const ids = listeners.get(args.event as string) ?? [];
            ids.push(args.handler as number);
            listeners.set(args.event as string, ids);
            return args.handler;
          }
          case "plugin:event|unlisten":
            return null;
          case "app_bootstrap":
            return {
              version: "0.1.2",
              settings,
              // A test that needs a collection sets `__mockLibrary` before the page loads.
              library: w.__mockLibrary ?? null,
              playback: {
                state: "playing",
                title: "Mock track",
                positionMs: 61000,
                durationMs: 200000,
                volume: 0.5,
              },
            };
          case "ui_sound":
            return null;
          case "playback_snapshot":
            return { state: "playing", title: "Mock track", volume: 0.5, signal: null };
          case "settings_get":
            return settings;
          case "settings_patch": {
            const patch = args.patch as { appearance?: Record<string, unknown> };
            Object.assign(settings.appearance, patch.appearance ?? {});
            return settings;
          }
          case "hotkeys_status":
          case "playlists_list":
          case "never_list":
          case "viz_videos":
            return [];
          case "autostart_available":
            return false;
          case "wave_season_card":
            return "preWinter";
          case "viz_start":
            return 1;
          case "viz_stop":
            return null;
          case "track_tags":
            return [];
          // Screens with a collection ask; without data the UI shows its fallbacks.
          case "track_feedback":
          case "viz_colors":
          case "artist_suggestions":
          case "collection_artists":
          case "collection_albums":
          case "storage_info":
          case "duplicates_count":
            throw { code: "NotFound", retryable: false, details: null };
          case "wave_start":
            return {
              status: "ok",
              data: { queued: 5, relaxed: [], noPopularity: false, few: false },
            };
          case "sleep_start":
          case "focus_start":
            return null;
          case "sleep_status":
            return { phase: "off", kind: null, remainingMs: 0, tracksLeft: null };
          case "focus_status":
            return { phase: "off", remainingMs: 0, segmentMs: 0, paused: false, today: 0 };
          case "sleep_caps":
            return { hibernate: true, modernStandby: true };
          case "fx_season_effect":
            return "snow";
          case "theme_state":
            return {
              id: "autumn",
              card: "preWinter",
              source: "season",
              seed: "#b2a832",
              viz: ["#b2a832", "#8f7a52", "#5a4a6c"],
              cover_variant: "autumn",
              effect: "leaves",
            };
          case "analysis_progress":
            return { done: 0, total: 0, paused: false };
          case "update_status":
            return { current: "0.1.2", justUpdated: null, notes: null };
          case "playback_toggle":
          case "playback_seek":
          case "playback_set_volume":
          case "playback_next":
          case "playback_prev":
          case "window_set_theme":
          case "import_cancel":
          case "library_confirm_missing":
          case "library_dismiss_question":
            return null;
          default:
            // An unmocked command fails loudly with its name (a silent `undefined` once cost
            // a 30 s timeout far from the cause).
            unknown.push(cmd);
            throw new Error(`mock: unknown command ${cmd}`);
        }
      },
    };
    w.__mock = {
      unknown,
      calls,
      emit: (event: string, payload: unknown) => {
        for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id: 0, payload });
      },
    };
  });
}

const calls = (page: Page, cmd: string) =>
  page.evaluate(
    (c) =>
      (
        window as unknown as { __mock: { calls: Array<{ cmd: string; args: unknown }> } }
      ).__mock.calls.filter((x) => x.cmd === c),
    cmd,
  );
const emit = (page: Page, event: string, payload: unknown) =>
  page.evaluate(
    ([e, p]) =>
      (window as unknown as { __mock: { emit: (e: unknown, p: unknown) => void } }).__mock.emit(
        e,
        p,
      ),
    [event, payload] as const,
  );

test.afterEach(async ({ page }) => {
  const unknown = await page.evaluate(
    () => (window as unknown as { __mock?: { unknown: string[] } }).__mock?.unknown ?? [],
  );
  expect(unknown, "commands the mock does not know").toEqual([]);
});

test("bootstrap hydrates settings and the player in one call", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  const footer = page.locator("footer");
  await expect(footer.getByText("Mock track")).toBeVisible();
  await expect(footer.locator(".t-timecode").first()).toHaveText("1:01");
  await expect(footer.locator(".t-timecode").last()).toHaveText("−2:19");
  await expect(footer.getByRole("button", { name: "Пауза" })).toBeEnabled();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "blue");
  expect(await calls(page, "app_bootstrap")).toHaveLength(1);
  const theme = await calls(page, "window_set_theme");
  expect(theme.at(-1)?.args).toEqual({ dark: true, palette: null });
});

test("commands go out, events come back, errors become a toast", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await expect(page.locator("footer").getByText("Mock track")).toBeVisible();

  await page.keyboard.press("Space");
  await expect.poll(async () => (await calls(page, "playback_toggle")).length).toBe(1);

  await emit(page, "playback-snapshot", {
    state: "paused",
    title: "Mock track",
    positionMs: 62000,
    durationMs: 200000,
    volume: 0.5,
  });
  await expect(page.locator("footer").getByRole("button", { name: "Слушать" })).toBeVisible();

  await emit(page, "playback-error", {
    error: { code: "DecodeFailed", retryable: false, details: null },
    file: "broken.flac",
  });
  await expect(page.getByRole("status").filter({ hasText: "broken.flac" })).toContainText(
    "Файл повреждён или не читается · broken.flac",
  );

  await page.getByRole("button", { name: "Настройки" }).click();
  await page.getByRole("button", { name: "Светлая" }).click();
  await expect
    .poll(async () => (await calls(page, "window_set_theme")).at(-1)?.args)
    .toEqual({ dark: false, palette: null });
  const patch = (await calls(page, "settings_patch")).at(-1)?.args as {
    patch: { appearance: { theme: string } };
  };
  expect(patch.patch.appearance.theme).toBe("light");
});

test("live effects only behind «Моя волна» (T4.20, FR-FX-006)", async ({ page }) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__mockLibrary = {
      root: "D:/Music",
      defaultRoot: "D:/Music",
      tracks: 12,
      artists: 3,
      albums: 2,
      unknown: 0,
      durationSec: 3000,
      bytes: 1,
      importing: false,
      offline: false,
      goneQuestion: null,
    };
  });
  await mockTauri(page);
  await page.goto("/");
  await expect(page.locator("footer").getByText("Mock track")).toBeVisible();
  await emit(page, "playback-snapshot", {
    state: "playing",
    trackId: 7,
    title: "Mock track",
    positionMs: 62000,
    durationMs: 200000,
    volume: 0.5,
  });
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  await expect(page.getByTestId("fx")).toHaveCount(1);
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          (window as unknown as { __sirtoneFx?: { stats: { type: string } } }).__sirtoneFx?.stats
            .type,
      ),
    )
    .toBe("snow");
  for (const name of ["Исполнители", "Альбомы", "Настройки"]) {
    await page.getByRole("button", { name, exact: true }).first().click();
    await expect(page.getByTestId("fx")).toHaveCount(0);
  }
});

test("sleep and focus start from the wave screen; their settings render (T4.21, T4.24)", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__mockLibrary = {
      root: "D:/Music",
      defaultRoot: "D:/Music",
      tracks: 12,
      artists: 3,
      albums: 2,
      unknown: 0,
      durationSec: 3000,
      bytes: 1,
      importing: false,
      offline: false,
      goneQuestion: null,
    };
  });
  await mockTauri(page);
  await page.goto("/");
  await expect(page.locator("footer").getByText("Mock track")).toBeVisible();
  await emit(page, "playback-snapshot", {
    state: "playing",
    trackId: 7,
    title: "Mock track",
    positionMs: 62000,
    durationMs: 200000,
    volume: 0.5,
  });
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  await page.getByRole("button", { name: "Сон-таймер", exact: true }).click();
  await page.getByRole("menuitem", { name: "Через 30 минут" }).click();
  await expect
    .poll(async () => (await calls(page, "sleep_start")).at(-1)?.args)
    .toEqual({ kind: "minutes", value: 30 });
  await page.getByRole("button", { name: "Фокус", exact: true }).click();
  await page.getByRole("menuitem", { name: "Работа 25 мин · перерыв 5 мин" }).click();
  await expect
    .poll(async () => (await calls(page, "focus_start")).at(-1)?.args)
    .toEqual({ work: 25, brk: 5 });
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  for (const h of ["Живые эффекты", "Сон-таймер", "Фокус"]) {
    await expect(page.getByRole("heading", { name: h, exact: true })).toBeVisible();
  }
  const text = await page.locator("main").innerText();
  expect(text).not.toMatch(/settings(Sleep|Focus|Fx)\.|\{count|\{time/);
  await expect(page.getByText("Предпочтительно без слов")).toBeVisible();
});

test("«Настроить волну»: tiles combine, «Слушать» starts and saves; no page bounce", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__mockLibrary = {
      root: "D:/Music",
      defaultRoot: "D:/Music",
      tracks: 12,
      artists: 3,
      albums: 2,
      unknown: 0,
      durationSec: 3000,
      bytes: 1,
      importing: false,
      offline: false,
      goneQuestion: null,
    };
  });
  await mockTauri(page);
  await page.goto("/");
  await expect(page.locator("footer").getByText("Mock track")).toBeVisible();
  expect(
    await page.evaluate(() => getComputedStyle(document.documentElement).overscrollBehaviorY),
  ).toBe("none");
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  await page.getByRole("button", { name: "Настроить", exact: true }).click();
  const dlg = page.getByRole("dialog");
  await dlg.getByRole("button", { name: "Работа", exact: true }).click();
  await dlg.getByRole("button", { name: "Спокойное", exact: true }).first().click();
  await dlg.getByRole("button", { name: "Без слов", exact: true }).click();
  await expect(dlg.getByText("Работаете?")).toBeVisible();
  await dlg.getByRole("button", { name: "Слушать", exact: true }).click();
  const call = (await calls(page, "wave_start")).at(-1)?.args as {
    query: { activity: string[]; character: string[]; language: string[] };
  };
  expect(call.query.activity).toEqual(["work"]);
  expect(call.query.character).toEqual(["calm"]);
  expect(call.query.language).toEqual(["none"]);
  const saved = (await calls(page, "settings_patch")).at(-1)?.args as {
    patch: { wave: { setup: { activity: string[] } } };
  };
  expect(saved.patch.wave.setup.activity).toEqual(["work"]);
  await expect(page.getByRole("button", { name: /Настроить\s*3/ })).toBeVisible();
});
