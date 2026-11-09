/**
 * Colour model of the app: `colorMath.ts` (pure math) bound to the tokens generated from
 * DESIGN.md (`tokens.generated.ts`, T9.1). Since 2.1 the user's colour is three tokens
 * (`accent`, `accentText`, `onAccent`); `primary*` are neutral theme tokens (D-155).
 */
import { COLOR, BACKGROUNDS as GEN_BG, INK_DARK, INK_LIGHT } from "../ui/tokens/tokens.generated";
import {
  type AccentContext,
  type AccentSet,
  generateAccentSetWith,
  inkOnWith,
  type Theme,
} from "./colorMath";

export * from "./colorMath";
export { INK_DARK, INK_LIGHT };

/** Surfaces the accent must work on (DESIGN.md §2: bg, surface, raised, flyout). */
export const BACKGROUNDS: Record<Theme, string[]> = {
  dark: [...GEN_BG.dark],
  light: [...GEN_BG.light],
};
/** `onPrimary` of each theme: text on a `primary` plate, ≥ 7:1. */
export const ON_PRIMARY: Record<Theme, string> = {
  dark: COLOR.dark.onPrimary,
  light: COLOR.light.onPrimary,
};
export const PRIMARY: Record<Theme, string> = {
  dark: COLOR.dark.primary,
  light: COLOR.light.primary,
};

const CTX: AccentContext = { backgrounds: BACKGROUNDS, inkDark: INK_DARK, inkLight: INK_LIGHT };

export const inkOn = (fill: string): string => inkOnWith(fill, CTX);
export const generateAccentSet = (seedHex: string, theme: Theme): AccentSet =>
  generateAccentSetWith(seedHex, theme, CTX);
