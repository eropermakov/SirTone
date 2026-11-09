import { generateAccentSet, type Theme } from "../lib/color";

/** Themes (DESIGN.md 2.1, FR-LOOK-001): «Авто» follows the system. The old owner palettes (D-74) are gone (D-155). */
export type ThemePref = "system" | Theme;
/** Stored values of removed themes (ember, mono, …) and anything unknown become «Авто» (D-155). */
export const normalizeTheme = (v: unknown): ThemePref =>
  v === "dark" || v === "light" || v === "system" ? v : "system";
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
export type AccentSource =
  | { kind: "preset"; slug: AccentSlug }
  | { kind: "custom"; hex: string }
  /** «Авто» (T2.2, DESIGN.md §4): the set picked from the playing cover. */
  | { kind: "auto" };

const media = () => window.matchMedia("(prefers-color-scheme: light)");
const contrastMedia = () => window.matchMedia("(prefers-contrast: more)");

export function resolveTheme(pref: ThemePref): Theme {
  if (pref !== "system") return pref;
  return media().matches ? "light" : "dark";
}

const CUSTOM_VARS = ["accent-fill", "accent-text", "accent-on"] as const;

/** Applies theme + accent to <html> with one write per change (docs/06 §4.2, D-58). */
export function applyAppearance(
  pref: ThemePref,
  source: AccentSource,
  /** For «Авто»: the set picked from the cover (Sir.kov until there is one). */
  autoSlug: AccentSlug = "sirkov",
): void {
  const accent: Exclude<AccentSource, { kind: "auto" }> =
    source.kind === "auto" ? { kind: "preset", slug: autoSlug } : source;
  const root = document.documentElement;
  const theme = resolveTheme(pref);
  root.dataset.theme = theme;
  // High contrast follows the system setting (lightHC / darkHC, FR-LOOK-001).
  if (contrastMedia().matches) root.dataset.contrast = "more";
  else delete root.dataset.contrast;
  if (accent.kind === "preset") {
    root.dataset.accent = accent.slug;
    for (const v of CUSTOM_VARS) root.style.removeProperty(`--${v}`);
  } else {
    root.dataset.accent = "custom";
    const s = generateAccentSet(accent.hex, theme);
    const values = [s.accentFill, s.accentText, s.accentOn];
    for (const [i, v] of CUSTOM_VARS.entries()) {
      root.style.setProperty(`--${v}`, values[i] ?? null);
    }
  }
}

/** Calls back when the OS switches light/dark or high contrast. */
export function onSystemThemeChange(cb: () => void): () => void {
  const ms = [media(), contrastMedia()];
  for (const m of ms) m.addEventListener("change", cb);
  return () => {
    for (const m of ms) m.removeEventListener("change", cb);
  };
}
