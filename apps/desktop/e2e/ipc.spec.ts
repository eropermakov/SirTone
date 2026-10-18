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
              playback: {
                state: "playing",
                title: "Mock track",
                positionMs: 61000,
                durationMs: 200000,
                volume: 0.5,
              },
            };
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
            return [];
          case "autostart_available":
            return false;
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
