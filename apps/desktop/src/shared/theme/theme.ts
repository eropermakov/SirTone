import { generateAccentSet, type Theme } from "../lib/color";

/** Owner palettes (D-74): complete dark themes with their own accent. */
export const PALETTES = ["ember", "mono"] as const;
export type Palette = (typeof PALETTES)[number];
export type ThemePref = "system" | Theme | Palette;
export const isPalette = (p: ThemePref): p is Palette =>
  (PALETTES as readonly string[]).includes(p);
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

export function resolveTheme(pref: ThemePref): Theme {
  if (isPalette(pref)) return "dark";
  if (pref !== "system") return pref;
  return media().matches ? "light" : "dark";
}

const CUSTOM_VARS = ["accent-fill", "accent-text", "accent-on", "primary", "primary-text"] as const;

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
  if (isPalette(pref)) {
    // The palette brings its own accent: the user's colour waits until another theme is chosen.
    root.dataset.palette = pref;
    for (const v of CUSTOM_VARS) root.style.removeProperty(`--${v}`);
    return;
  }
  delete root.dataset.palette;
  if (accent.kind === "preset") {
    root.dataset.accent = accent.slug;
    for (const v of CUSTOM_VARS) root.style.removeProperty(`--${v}`);
  } else {
    root.dataset.accent = "custom";
    const s = generateAccentSet(accent.hex, theme);
    const values = [s.accentFill, s.accentText, s.accentOn, s.primary, s.primaryText];
    for (const [i, v] of CUSTOM_VARS.entries()) {
      root.style.setProperty(`--${v}`, values[i] ?? null);
    }
  }
}

/** Calls back when the OS switches light/dark (used while pref = "system"). */
export function onSystemThemeChange(cb: () => void): () => void {
  const m = media();
  m.addEventListener("change", cb);
  return () => m.removeEventListener("change", cb);
}
