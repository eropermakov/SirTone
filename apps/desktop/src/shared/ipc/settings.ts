import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type AppError, commands } from "./bindings";

/**
 * Settings IPC (T0.6). Commands come from the generated `bindings.ts` (T0.7, D-09); the settings
 * document crosses IPC as opaque JSON, so its UI-side shape is declared here and validated in Rust.
 * Outside Tauri (browser preview, Playwright) a localStorage fallback keeps the UI usable.
 */
export type SettingsDoc = {
  appearance: {
    theme: "system" | "dark" | "light";
    accent: {
      source: "preset" | "auto" | "custom" | "seasonal";
      preset: string;
      custom: string | null;
    };
  };
  [key: string]: unknown;
};
export type SettingsPatch = {
  appearance?: { theme?: string; accent?: Record<string, unknown>; seasonal_theme?: boolean };
  hotkeys?: Record<string, string | null>;
  performance?: { level?: number; overrides?: Record<string, unknown> };
  onboarding?: { done?: boolean; tips_seen?: boolean };
  wave?: { learning?: boolean; novelty?: number };
  library?: { organize_external?: "suggest" | "auto" | "never" };
  network?: { offline_mode?: boolean };
  viz?: { mode?: "waves" | "milkdrop" | "video"; follow_theme_accent?: boolean; minimal?: boolean };
};

const WEB_KEY = "sirtone.web.settings";
const LEGACY_PROTO_KEY = "sirtone.proto.appearance";
const WEB_DEFAULT: SettingsDoc = {
  appearance: { theme: "system", accent: { source: "preset", preset: "sirkov", custom: null } },
};

function webRead(): SettingsDoc {
  try {
    const raw = localStorage.getItem(WEB_KEY);
    return raw ? (JSON.parse(raw) as SettingsDoc) : structuredClone(WEB_DEFAULT);
  } catch {
    return structuredClone(WEB_DEFAULT);
  }
}

function merge(
  base: Record<string, unknown>,
  patch: Record<string, unknown>,
): Record<string, unknown> {
  const out: Record<string, unknown> = { ...base };
  for (const [k, v] of Object.entries(patch)) {
    const b = out[k];
    out[k] =
      v && typeof v === "object" && !Array.isArray(v) && b && typeof b === "object"
        ? merge(b as Record<string, unknown>, v as Record<string, unknown>)
        : v;
  }
  return out;
}

/** Rejection value of IPC calls: the code is stable, the UI turns it into text (docs/03 §5.5). */
export class IpcError extends Error {
  constructor(readonly error: AppError) {
    super(error.code);
  }
}

export async function getSettings(): Promise<SettingsDoc> {
  if (!isTauri()) return webRead();
  return (await commands.settingsGet()) as SettingsDoc;
}

export async function patchSettings(patch: SettingsPatch): Promise<SettingsDoc> {
  if (!isTauri()) {
    const next = merge(webRead(), patch as Record<string, unknown>) as SettingsDoc;
    try {
      localStorage.setItem(WEB_KEY, JSON.stringify(next));
    } catch {
      // storage unavailable: the change still applies for this session
    }
    return next;
  }
  const res = await commands.settingsPatch(patch);
  if (res.status === "error") throw new IpcError(res.error);
  return res.data as SettingsDoc;
}

export async function onSettingsChanged(cb: () => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  return listen("settings://changed", cb);
}

/**
 * One-time move of prototype 0.1 appearance (localStorage) into real settings, so the theme and
 * colour the owner picked survive the upgrade.
 */
export async function migrateLegacyAppearance(): Promise<void> {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(LEGACY_PROTO_KEY);
  } catch {
    return;
  }
  if (!raw) return;
  try {
    const old = JSON.parse(raw) as {
      theme?: string;
      accent?: { kind: "preset"; slug: string } | { kind: "custom"; hex: string };
    };
    const accent =
      old.accent?.kind === "custom"
        ? { source: "custom", custom: old.accent.hex }
        : old.accent?.kind === "preset"
          ? { source: "preset", preset: old.accent.slug }
          : undefined;
    await patchSettings({
      appearance: { ...(old.theme ? { theme: old.theme } : {}), ...(accent ? { accent } : {}) },
    });
    localStorage.removeItem(LEGACY_PROTO_KEY);
  } catch {
    // malformed legacy value: ignore, defaults stay
  }
}
