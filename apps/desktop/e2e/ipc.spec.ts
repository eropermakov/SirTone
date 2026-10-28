import AxeBuilder from "@axe-core/playwright";
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
            const patch = args.patch as {
              appearance?: Record<string, unknown>;
              wave?: Record<string, unknown>;
            };
            Object.assign(settings.appearance, patch.appearance ?? {});
            if (patch.wave) {
              const s = settings as unknown as { wave?: Record<string, unknown> };
              s.wave = { ...(s.wave ?? {}), ...patch.wave };
            }
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
          case "queue_get":
            return {
              items: [],
              position: 0,
              repeat: "off",
              shuffle: false,
              autoplay: null,
              playlist: null,
            };
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
          case "wave_composition":
            return {
              season: "autumn",
              daypart: "evening",
              weather: "rain",
              event: null,
              eventDays: 0,
              off: { season: false, context: false, event: false, taste: false },
            };
          case "wave_set_off":
            return null;
          case "collection_genres":
            return [{ key: "rock", name: "Rock", tracks: 3 }];
          case "collection_tracks": {
            // «Все треки» (D-127): two keyset pages by title, as Rust sends them.
            const row = (
              id: number,
              title: string,
              artists: string,
              album: string | null,
              s: number,
            ) => ({
              id,
              title,
              artists,
              album,
              durationMs: s * 1000,
              albumId: album ? id : null,
              artistId: id,
            });
            return args.after == null
              ? {
                  rows: [row(3, "Арфа", "Яблоко", "Сад", 240), row(1, "Берег", "Бриз", null, 95)],
                  next: { key: "берег", id: 1 },
                }
              : { rows: [row(2, "Вьюга", "Аврора", "Зима", 180)], next: null };
          }
          case "play_tracks":
            return null;
          case "bt_devices":
            return [
              { name: "WH-1000XM4", connected: false, endpoints: ["{bt-1}"] },
              { name: "JBL Flip", connected: true, endpoints: ["{dev-2}"] },
            ];
          case "bt_set":
            return true;
          case "bt_open_settings":
            return null;
          case "library_pick_folder":
            return "E:/Export";
          case "export_start":
            return { files: 3, bytes: 300 };
          case "export_cancel":
            return null;
          case "legal_notices":
            return {
              rust: 400,
              js: 25,
              sections: [
                {
                  title: "Шрифт Geologica",
                  license: "SIL Open Font License 1.1",
                  items: [],
                  text: "SIL OPEN FONT LICENSE Version 1.1",
                },
                { title: "MIT", license: "MIT", items: ["serde 1.0"], text: "MIT License text" },
              ],
            };
          case "audio_devices":
            return [
              { id: "{dev-1}", name: "Динамики (Realtek)", isDefault: true },
              { id: "{dev-2}", name: "Наушники USB", isDefault: false },
            ];
          case "wave_query_count":
            return { tracks: 12, relaxed: false };
          case "wave_by":
            return null;
          case "wave_context_name":
            return {
              daypart: "evening",
              weather: "snow",
              season: "autumn",
              nextAt: Date.now() + 3_600_000,
            };
          case "event_offer":
            return (
              (window as unknown as Record<string, unknown>).__mockEvent ?? {
                id: null,
                daysToPeak: 0,
                strong: false,
                atmospheric: false,
              }
            );
          case "event_answer":
          case "event_refresh":
            return null;
          case "event_list":
            return [
              { id: "halloween", group: "secular", enabled: true, offReason: null },
              {
                id: "first_snow",
                group: "micro_weather",
                enabled: false,
                offReason: "declined_3x",
              },
            ];
          case "weather_get":
            return {
              class: null,
              tempC: null,
              isDay: true,
              drift: 0,
              intensity: 0,
              fetchedAt: null,
              fresh: false,
            };
          case "city_search":
            // typedError wraps the bare answer itself.
            return [{ name: "Москва", country: "Россия", admin: "Москва", lat: 55.75, lon: 37.62 }];
          case "sleep_start":
          case "focus_start":
            return null;
          case "sleep_status":
            return { phase: "off", kind: null, remainingMs: 0, tracksLeft: null };
          case "focus_status":
            return { phase: "off", remainingMs: 0, segmentMs: 0, paused: false, today: 0 };
          case "sleep_caps":
            return { hibernate: true, modernStandby: true };
          case "fx_sources":
            return { season: "snow", weather: null, event: null, drift: 0, intensity: 0 };
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
  await expect(dlg.getByTestId("wave-count")).toHaveText("Подходит 12 треков");
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

test("«Место и погода»: a city is found and chosen (T5.5)", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  await expect(page.getByTestId("place-city")).toHaveText("Город не выбран");
  await page.getByRole("searchbox", { name: "Найти город" }).fill("Моск");
  await page.getByRole("button", { name: /Москва/ }).click();
  await expect
    .poll(async () => {
      const last = (await calls(page, "settings_patch")).at(-1)?.args as {
        patch: { context?: { city?: { name: string; lat: number } } };
      };
      return last?.patch.context?.city?.name;
    })
    .toBe("Москва");
  await expect(page.getByTestId("place-city")).toContainText("Москва");
});

test("event card: «Скоро Хэллоуин», answers go to Rust; «События» lists every event (T5.14)", async ({
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
    (window as unknown as Record<string, unknown>).__mockEvent = {
      id: "halloween",
      daysToPeak: 5,
      strong: false,
      atmospheric: true,
    };
  });
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  // The contextual name (T5.11): the weather and the part of the day, agreed.
  await expect(page.getByTestId("wave-context")).toContainText("Снежный вечер");
  await expect(page.getByTestId("wave-context")).toContainText("Обновится в");
  const card = page.getByTestId("event-card");
  await expect(card).toContainText("Скоро Хэллоуин");
  await expect(card).toContainText("мало подходящих треков");
  await card.getByRole("button", { name: "Ещё" }).click();
  await page.getByRole("menuitem", { name: "Не в этом году" }).click();
  await expect
    .poll(async () => (await calls(page, "event_answer")).at(-1)?.args)
    .toEqual({ id: "halloween", answer: "year" });
  await expect(card).toHaveCount(0);
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  await expect(page.getByRole("heading", { name: "События", exact: true })).toBeVisible();
  await expect(page.getByTestId("event-first_snow")).toContainText("три раза подряд");
  await page.getByTestId("event-halloween").getByRole("switch").click();
  await expect
    .poll(async () => {
      const last = (await calls(page, "settings_patch")).at(-1)?.args as {
        patch: { events?: { enabled?: Record<string, boolean> } };
      };
      return last?.patch.events?.enabled?.halloween;
    })
    .toBe(false);
  const text = await page.locator("main").innerText();
  expect(text).not.toMatch(/settingsEvents\.|events\.name/);
});

test("«Состав волны»: chips under the player switch an influence off (T5.16)", async ({ page }) => {
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
    positionMs: 1000,
    durationMs: 200000,
    volume: 0.5,
    waveProfile: "my_wave",
  });
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  const chips = page.getByTestId("wave-chips");
  for (const c of ["Осень", "Вечер", "Дождь", "Твой вкус"]) {
    await expect(chips.getByRole("button", { name: c, exact: true })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  }
  await chips.getByRole("button", { name: "Осень", exact: true }).click();
  await expect
    .poll(async () => (await calls(page, "wave_set_off")).at(-1)?.args)
    .toEqual({ off: { season: true, context: false, event: false, taste: false } });
  await expect(chips.getByRole("button", { name: "Осень", exact: true })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  // «Настроить волну» 2.0: mode, novelty, influence, ranges.
  await page.getByRole("button", { name: /^Настроить/ }).click();
  const dlg = page.getByRole("dialog");
  for (const h of ["Режим", "Знакомое ↔ Неожиданное", "Сила влияния", "Энергия и темп"]) {
    await expect(dlg.getByRole("heading", { name: h, exact: true })).toBeVisible();
  }
  await dlg.getByRole("button", { name: "Любимое", exact: true }).click();
  await expect(dlg.getByText("ползунок не действует")).toBeVisible();
  expect(await dlg.innerText()).not.toMatch(/wave\.setup\./);
});

test("«Мои волны»: a setup saved with a name becomes a card; rename (T5.17)", async ({ page }) => {
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
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  await page.getByRole("button", { name: /^Настроить/ }).click();
  const dlg = page.getByRole("dialog");
  await dlg.getByRole("button", { name: "Спокойное", exact: true }).first().click();
  await dlg.getByRole("button", { name: "Сохранить как свою волну" }).click();
  const name = page.getByRole("dialog").filter({ hasText: "Название" });
  await name.getByRole("textbox").fill("Вечерняя тишина");
  await name.getByRole("button", { name: "Сохранить", exact: true }).click();
  const card = page.getByRole("button", { name: "Вечерняя тишина", exact: true });
  await expect(card).toBeVisible();
  const saved = (await calls(page, "settings_patch")).at(-1)?.args as {
    patch: { wave: { mine: { name: string; query: { character?: string[] } }[] } };
  };
  expect(saved.patch.wave.mine[0]?.name).toBe("Вечерняя тишина");
  await card.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Переименовать" }).click();
  const ren = page.getByRole("dialog").filter({ hasText: "Название" });
  await ren.getByRole("textbox").fill("Тихо");
  await ren.getByRole("button", { name: "Сохранить", exact: true }).click();
  await expect(page.getByRole("button", { name: "Тихо", exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Тихо", exact: true }).click();
  await expect.poll(async () => (await calls(page, "wave_start")).length).toBeGreaterThan(0);
});

test("«Устройство вывода»: a device by name, exclusive and pause switches (T5.8)", async ({
  page,
}) => {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  const list = page.getByTestId("output-devices");
  await expect(list.getByRole("button", { name: "Как в Windows" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await list.getByRole("button", { name: "Наушники USB" }).click();
  await expect
    .poll(async () => {
      const last = (await calls(page, "settings_patch")).at(-1)?.args as {
        patch: { playback?: { output?: { device?: string } } };
      };
      return last?.patch.playback?.output?.device;
    })
    .toBe("{dev-2}");
  await page.getByRole("switch", { name: "Монопольный режим" }).click();
  await expect
    .poll(async () => {
      const last = (await calls(page, "settings_patch")).at(-1)?.args as {
        patch: { playback?: { output?: { exclusive?: boolean } } };
      };
      return last?.patch.playback?.output?.exclusive;
    })
    .toBe(true);
  await expect(page.getByText("не приняло монопольный режим")).toBeVisible();
});

test("«Экспорт»: the whole library into a folder, progress, done (T5.9)", async ({ page }) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__mockLibrary = {
      root: "D:/Music",
      defaultRoot: "D:/Music",
      tracks: 3,
      artists: 1,
      albums: 1,
      unknown: 0,
      durationSec: 600,
      bytes: 300,
      importing: false,
      offline: false,
      goneQuestion: null,
    };
  });
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  await page.getByRole("button", { name: "Экспортировать всю медиатеку" }).click();
  const dlg = page.getByTestId("export-dialog");
  await expect(dlg.getByRole("button", { name: "Экспортировать", exact: true })).toBeDisabled();
  await dlg.getByRole("button", { name: "Выбрать папку" }).click();
  await expect(page.getByTestId("export-dest")).toHaveText("E:/Export");
  await dlg.getByRole("button", { name: "Экспортировать", exact: true }).click();
  await expect
    .poll(async () => (await calls(page, "export_start")).at(-1)?.args)
    .toEqual({
      req: { kind: "all", id: null, dest: "E:/Export", keepStructure: true, number: false },
    });
  await expect(page.getByText("Скопировано 0 из 3")).toBeVisible();
  await emit(page, "export-progress", {
    done: 3,
    total: 3,
    bytes: 300,
    totalBytes: 300,
    skipped: 0,
    finished: true,
    cancelled: false,
    error: null,
  });
  await expect(page.getByText("Скопировано 3 файла")).toBeVisible();
  await expect(page.getByTestId("export-dialog")).toHaveCount(0);
});

test("Bluetooth in the player: paired devices, connect, play through (T5.4)", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Bluetooth", exact: true }).click();
  // The menu grows up from the bottom bar and stays inside the window (owner, proto 32).
  const box = await page.getByRole("menu", { name: "Bluetooth" }).boundingBox();
  const vh = page.viewportSize()?.height ?? 0;
  expect(box && box.y >= 0 && box.y + box.height <= vh).toBe(true);
  await page.getByRole("menuitem", { name: "WH-1000XM4" }).click();
  await page.getByRole("menuitem", { name: "Подключить" }).click();
  await expect
    .poll(async () => (await calls(page, "bt_set")).at(-1)?.args)
    .toEqual({ name: "WH-1000XM4", connect: true });
  await page.getByRole("button", { name: "Bluetooth", exact: true }).click();
  await page.getByRole("menuitem", { name: "JBL Flip" }).click();
  await page.getByRole("menuitem", { name: "Играть через него" }).click();
  await expect
    .poll(async () => {
      const last = (await calls(page, "settings_patch")).at(-1)?.args as {
        patch: { playback?: { output?: { device?: string } } };
      };
      return last?.patch.playback?.output?.device;
    })
    .toBe("{dev-2}");
});

test("the mouse wheel over the volume turns it, 5 % a notch (owner, proto 32)", async ({
  page,
}) => {
  await mockTauri(page);
  await page.goto("/");
  const vol = page.getByRole("slider", { name: "Громкость" });
  await vol.hover();
  await page.mouse.wheel(0, 100);
  await expect
    .poll(async () => (await calls(page, "playback_set_volume")).at(-1)?.args)
    .toEqual({ volume: 0.45 });
  await page.mouse.wheel(0, -100);
  await page.mouse.wheel(0, -100);
  await expect
    .poll(async () => (await calls(page, "playback_set_volume")).at(-1)?.args)
    .toEqual({ volume: 0.55 });
});

test("«Язык»: English switches the UI at once and is saved (T6.5)", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  await page.getByRole("button", { name: "English", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Language", exact: true })).toBeVisible();
  await expect
    .poll(async () => {
      const last = (await calls(page, "settings_patch")).at(-1)?.args as {
        patch: { language?: string };
      };
      return last?.patch.language;
    })
    .toBe("en");
  await page.getByRole("button", { name: "Русский", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Язык", exact: true })).toBeVisible();
});

test("«Лицензии»: the font, models and every package with its text (T6.6)", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  await page.getByRole("button", { name: "Лицензии", exact: true }).click();
  const list = page.getByTestId("licenses");
  await expect(list).toContainText("400 библиотек Rust");
  await list.getByRole("button", { name: /Шрифт Geologica/ }).click();
  await expect(list).toContainText("SIL OPEN FONT LICENSE");
  await expect(list.getByRole("button", { name: /serde 1\.0/ })).toBeVisible();
});

test("WCAG AA on the new UI: wave screen, dialogs, menus (T7.5)", async ({ page }) => {
  await page.addInitScript(() => {
    const w = window as unknown as Record<string, unknown>;
    w.__mockLibrary = {
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
    w.__mockEvent = { id: "halloween", daysToPeak: 5, strong: true, atmospheric: false };
  });
  await mockTauri(page);
  await page.goto("/");
  await expect(page.locator("footer").getByText("Mock track")).toBeVisible();
  await emit(page, "playback-snapshot", {
    state: "playing",
    trackId: 7,
    title: "Mock track",
    positionMs: 1000,
    durationMs: 200000,
    volume: 0.5,
    waveProfile: "my_wave",
  });
  const settled = () =>
    page.evaluate(() =>
      Promise.all(document.getAnimations().map((a) => a.finished.catch(() => undefined))),
    );
  const check = async (where: string) => {
    await settled();
    const r = await new AxeBuilder({ page }).analyze();
    const bad = r.violations.filter((v) => v.impact === "critical" || v.impact === "serious");
    expect(bad.map((v) => `${where}: ${v.id} — ${v.help} (${v.nodes[0]?.target})`)).toEqual([]);
  };
  await page.getByRole("button", { name: "Моя волна", exact: true }).first().click();
  await expect(page.getByTestId("wave-chips")).toBeVisible();
  await check("wave");
  await page.getByRole("button", { name: /^Настроить/ }).click();
  await page.getByRole("button", { name: "Для знатоков" }).click();
  await check("setup");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Bluetooth", exact: true }).click();
  await check("bluetooth menu");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  await check("settings");
  await page.getByRole("button", { name: "Лицензии", exact: true }).click();
  await check("licenses");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Экспортировать всю медиатеку" }).click();
  await check("export");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("export-dialog")).toHaveCount(0);
  await page.keyboard.press("F1");
  await expect(page.getByTestId("help")).toBeVisible();
  await check("help");
});

test("«Справка»: F1 and Settings → «Справка», one section at a time (T7.8)", async ({ page }) => {
  await mockTauri(page);
  await page.goto("/");
  await expect(page.locator("footer")).toBeVisible();
  await page.keyboard.press("F1");
  const help = page.getByTestId("help");
  await expect(help).toContainText("Перетащите папку");
  await help.getByRole("button", { name: "Клавиши" }).click();
  await expect(help.getByRole("button", { name: "Клавиши" })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(help).toContainText("Ctrl+Alt+Пробел");
  await page.keyboard.press("Escape");
  await expect(help).toHaveCount(0);
  await page.getByRole("button", { name: "Настройки", exact: true }).first().click();
  await page.getByRole("button", { name: "Справка", exact: true }).click();
  await expect(help).toContainText("Перетащите папку");
});

test("«Все треки»: the sidebar opens every track; sort and play the sorted list (D-127)", async ({
  page,
}) => {
  await page.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__mockLibrary = {
      root: "D:/Music",
      defaultRoot: "D:/Music",
      tracks: 3,
      artists: 3,
      albums: 2,
      unknown: 0,
      durationSec: 515,
      bytes: 300,
      importing: false,
      offline: false,
      goneQuestion: null,
    };
  });
  await mockTauri(page);
  await page.goto("/");
  await page.getByRole("button", { name: "Все треки", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Все треки" })).toBeVisible();
  const list = page.getByRole("list", { name: "Все треки" });
  const titles = () => list.locator("li .t-body").allTextContents();
  // Both pages arrived, in the server's title order.
  await expect.poll(titles).toEqual(["Арфа", "Берег", "Вьюга"]);
  expect((await calls(page, "collection_tracks")).map((c) => c.args)).toEqual([
    { after: null, limit: 500 },
    { after: { key: "берег", id: 1 }, limit: 500 },
  ]);

  await page.getByRole("button", { name: "По названию" }).click();
  await page.getByRole("menuitem", { name: "По длительности" }).click();
  await expect.poll(titles).toEqual(["Берег", "Вьюга", "Арфа"]);
  // The same option again reverses (Apple Music, F-34).
  await page.getByRole("button", { name: "По длительности" }).click();
  await page.getByRole("menuitem", { name: "По длительности" }).click();
  await expect.poll(titles).toEqual(["Арфа", "Вьюга", "Берег"]);
  await page.getByRole("button", { name: "По длительности" }).click();
  await page.getByRole("menuitem", { name: "По исполнителю" }).click();
  await expect.poll(titles).toEqual(["Вьюга", "Берег", "Арфа"]);

  // Double click plays the sorted list from that row.
  await list.getByRole("button", { name: /Берег/ }).dblclick();
  await expect
    .poll(async () => (await calls(page, "play_tracks")).at(-1)?.args)
    .toEqual({ ids: [2, 1, 3], start: 1, context: "collection", playlist: null });
  await page.locator("main").getByRole("button", { name: "Слушать", exact: true }).click();
  await expect
    .poll(async () => (await calls(page, "play_tracks")).at(-1)?.args)
    .toEqual({ ids: [2, 1, 3], start: 0, context: "collection", playlist: null });
});
