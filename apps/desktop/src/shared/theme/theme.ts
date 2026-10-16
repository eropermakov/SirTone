import { generateAccentSet, type Theme } from "../lib/color";

export type ThemePref = "system" | Theme;
/** 17 owner sets (DESIGN.md §4) in AccentPicker order: cold → neutral → warm → red. */
export const ACCENT_SLUGS = [
  "sirkov",
  "blue",
  "violet",
  "lilac",
  "lavender",
  "gray",
  "black",
  "yellow",
  "orange",
  "ginger",
  "coral",
  "brick",
  "lightred",
  "red",
  "darkred",
  "burgundy",
  "bordeaux",
] as const;
export type AccentSlug = (typeof ACCENT_SLUGS)[number];
export type AccentSource = { kind: "preset"; slug: AccentSlug } | { kind: "custom"; hex: string };

const media = () => window.matchMedia("(prefers-color-scheme: light)");

export function resolveTheme(pref: ThemePref): Theme {
  if (pref !== "system") return pref;
  return media().matches ? "light" : "dark";
}

const CUSTOM_VARS = ["accent-fill", "accent-text", "accent-on", "primary", "primary-text"] as const;

/** Applies theme + accent to <html> with one write per change (docs/06 §4.2, D-58). */
export function applyAppearance(pref: ThemePref, accent: AccentSource): void {
  const root = document.documentElement;
  const theme = resolveTheme(pref);
  root.dataset.theme = theme;
  if (accent.kind === "preset") {
    root.dataset.accent = accent.slug;
    for (const v of CUSTOM_VARS) root.style.removeProperty(`--${v}`);
  } else {
    root.dataset.accent = "custom";
    const s = generateAccentSet(accent.hex, theme);
    const values = [s.accentFill, s.accentText, s.accentOn, s.primary, s.primaryText];
    CUSTOM_VARS.forEach((v, i) => root.style.setProperty(`--${v}`, values[i] ?? null));
  }
}

/** Calls back when the OS switches light/dark (used while pref = "system"). */
export function onSystemThemeChange(cb: () => void): () => void {
  const m = media();
  m.addEventListener("change", cb);
  return () => m.removeEventListener("change", cb);
}
