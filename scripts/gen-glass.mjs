// «Стекло» (D-156, T9.5): the three levels of DESIGN.md `glass`, per theme, for gen-tokens.mjs.
// The panel is `bg` at the level's opacity over whatever is behind it. The level is raised when
// `text` on the panel would fall below 4.5:1 over the worst backdrop (pure white on dark, pure
// black on light) — «плотность поднимается автоматически». `textMuted` is not held to it (docs/10 §7).
import { contrast, hexToRgb, rgbToHex } from "../apps/desktop/src/shared/lib/colorMath.ts";

export const LEVELS = ["transparent", "medium", "matte"];

export function glassLevels(t) {
  const out = {};
  for (const level of LEVELS) {
    out[level] = {};
    for (const theme of ["dark", "light"]) {
      const c = t.color[theme];
      const worst = hexToRgb(theme === "dark" ? "#ffffff" : "#000000");
      const panel = hexToRgb(c.bg);
      const over = (a) =>
        rgbToHex({
          r: panel.r * a + worst.r * (1 - a),
          g: panel.g * a + worst.g * (1 - a),
          b: panel.b * a + worst.b * (1 - a),
        });
      let a = t.glass[level];
      while (a < 100 && contrast(c.text, over(a / 100)) < 4.5) a++;
      out[level][theme] = { set: t.glass[level], effective: a };
    }
  }
  return out;
}

export function glassCss(t) {
  const g = glassLevels(t);
  let css =
    "\n/* Glass (D-156): panels over the background; `.glass` = bg at the level's opacity + blur */\n";
  css += `:root {\n  --glass-blur: ${t.glass.blur}px;\n}\n`;
  for (const theme of ["dark", "light"]) {
    const base = theme === "dark" ? ':root,\n[data-theme="dark"]' : '[data-theme="light"]';
    css += `${base} {\n  --glass-alpha: ${g.medium[theme].effective}%;\n}\n`;
    for (const level of LEVELS) {
      const sel =
        theme === "dark"
          ? `[data-theme="dark"][data-glass="${level}"],\n:root:not([data-theme])[data-glass="${level}"]`
          : `[data-theme="light"][data-glass="${level}"]`;
      css += `${sel} {\n  --glass-alpha: ${g[level][theme].effective}%;\n}\n`;
    }
  }
  css +=
    ".glass {\n  background: color-mix(in srgb, var(--bg-base) var(--glass-alpha), transparent);\n";
  css += "  backdrop-filter: blur(var(--glass-blur));\n}\n";
  css +=
    '/* The dense fallback: auto-degrade / weak presets (data-blur="off"), high contrast, reduced transparency */\n';
  css +=
    ':root[data-blur="off"] .glass,\n:root[data-contrast="more"] .glass {\n  background: var(--bg-base);\n  backdrop-filter: none;\n}\n';
  css +=
    "@media (prefers-reduced-transparency: reduce) {\n  .glass {\n    background: var(--bg-base);\n    backdrop-filter: none;\n  }\n}\n";
  return css;
}
