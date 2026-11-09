#!/usr/bin/env node
// Design tokens for the phones (D-141, M1.2; D-155 / T9.1 for design 2.1).
// The single source is the `sirkov-tokens` block of DESIGN.md. `scripts/gen-tokens.mjs` turns it
// into the desktop CSS; this script takes the same data (colours from DESIGN.md, metrics and text
// styles from the generated CSS) into `design/tokens.json`, the machine copy the phones read:
//   --extract    writes design/tokens.json;
//   --check      fails if tokens.json differs from DESIGN.md + the desktop CSS (the look never drifts);
//   --gen        writes the Swift and Kotlin token files;
//   --check-gen  --check, and the generated files are current (gate).
// Colours are never copied by hand.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildAccentSets, NEUTRAL_VARS, parseTokens } from "./gen-tokens.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const TOK = join(ROOT, "apps/desktop/src/shared/ui/tokens");
const DESIGN = join(ROOT, "DESIGN.md");
const SEEDS = join(ROOT, "apps/desktop/src/shared/theme/accentSeeds.json");
const CSS_FILES = [join(TOK, "sirtone.css"), join(TOK, "extra.css")];
const JSON_PATH = join(ROOT, "design/tokens.json");
const SWIFT = join(ROOT, "apps/mobile-ios/Sources/SirToneDesign/Tokens.swift");
const KOTLIN = join(ROOT, "apps/mobile-android/design/Tokens.kt");
const THEMES = ["dark", "light", "darkHC", "lightHC"];

/** CSS → list of { selector, vars, props } blocks (comments dropped, selectors normalised). */
function parseCss(text) {
  const src = text.replace(/\/\*[\s\S]*?\*\//g, "");
  const blocks = [];
  for (const m of src.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selector = m[1].replace(/\s+/g, " ").trim();
    const vars = {};
    const props = {};
    for (const decl of m[2].split(";")) {
      const i = decl.indexOf(":");
      if (i < 0) continue;
      const k = decl.slice(0, i).trim();
      const v = decl
        .slice(i + 1)
        .replace(/\s+/g, " ")
        .trim();
      if (!k) continue;
      if (k.startsWith("--")) vars[k.slice(2)] = v;
      else props[k] = v;
    }
    blocks.push({ selector, vars, props });
  }
  return blocks;
}

const hexRgb = (h) => [1, 3, 5].map((i) => Number.parseInt(h.slice(i, i + 2), 16));
const toHex = (c) => `#${c.map((v) => Math.round(v).toString(16).padStart(2, "0")).join("")}`;
/** `color-mix(in srgb, a t%, b)`. */
const mix = (a, b, t) => {
  const x = hexRgb(a);
  const y = hexRgb(b);
  return toHex(x.map((v, i) => v * t + y[i] * (1 - t)));
};

/** Neutrals of one theme: what the CSS declares, with the derived ones computed (no new hex). */
function neutrals(c) {
  const out = {};
  for (const [v, k] of NEUTRAL_VARS) out[v] = c[k].toLowerCase();
  const [r, g, b] = hexRgb(c.text);
  out["bg-hover"] = `rgba(${r}, ${g}, ${b}, 0.06)`;
  out["bg-selected"] = out.primary;
  out.inactive = mix(out["text-secondary"], out["bg-base"], 0.6);
  out["artwork-placeholder"] = out["bg-flyout"];
  out["artwork-scrim"] = "rgba(0, 0, 0, 0.45)";
  return out;
}

/** The machine copy: base metrics + text styles from the CSS, colours from DESIGN.md. */
function structure() {
  const tokens = parseTokens(readFileSync(DESIGN, "utf8"));
  const seeds = JSON.parse(readFileSync(SEEDS, "utf8"));
  const t = { base: {}, neutral: {}, accent: {}, type: {} };
  for (const file of CSS_FILES) {
    for (const b of parseCss(readFileSync(file, "utf8"))) {
      if (b.selector === ":root") Object.assign(t.base, b.vars);
      for (const sel of b.selector.split(",").map((s) => s.trim())) {
        if (/^\.t-[a-z-]+$/.test(sel) && b.props.font) {
          const f = b.props.font.match(/^(\d+) (\d+)px \/ (\d+)px/);
          if (f) t.type[sel.slice(3)] = { weight: +f[1], size: +f[2], line: +f[3] };
        }
      }
    }
  }
  for (const th of THEMES) t.neutral[th] = neutrals(tokens.color[th]);
  const { sets } = buildAccentSets(tokens, seeds);
  for (const [slug, themes] of Object.entries(sets)) {
    t.accent[slug] = {};
    for (const th of ["dark", "light"]) {
      t.accent[slug][th] = {
        "accent-fill": themes[th].accentFill,
        "accent-text": themes[th].accentText,
        "accent-on": themes[th].accentOn,
      };
    }
  }
  return t;
}

// ---- generation for the phones -------------------------------------------------------------

/** "#rrggbb" or "rgba(r, g, b, a)" → [r, g, b, a] (0…1), or null for non-colours. */
function rgba(v) {
  let m = v.match(/^#([0-9a-f]{6})$/i);
  if (m) {
    const n = Number.parseInt(m[1], 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  m = v.match(/^rgba\((\d+), (\d+), (\d+), ([\d.]+)\)$/);
  return m ? [+m[1], +m[2], +m[3], +m[4]] : null;
}
const camel = (s) => s.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase()).replace(/^(\d)/, "n$1");
const px = (v) => (/^\d+(\.\d+)?px$/.test(v) ? Number.parseFloat(v) : null);
const ms = (v) => (/^\d+ms$/.test(v) ? Number.parseInt(v, 10) : null);
const HEADER = "// Generated by scripts/tokens.mjs from DESIGN.md (D-141, T9.1). Do not edit.\n";
const cap = (s) => s[0].toUpperCase() + s.slice(1);

function swiftColor(c) {
  const f = (x) => (x / 255).toFixed(4);
  return `Color(.sRGB, red: ${f(c[0])}, green: ${f(c[1])}, blue: ${f(c[2])}, opacity: ${c[3]})`;
}
function genSwift(t) {
  const L = [HEADER, "import SwiftUI", "", "public enum SirToneTokens {"];
  L.push("    public enum Metric {");
  for (const [k, v] of Object.entries(t.base)) {
    const n = px(v) ?? ms(v);
    if (n !== null) L.push(`        public static let ${camel(k)}: CGFloat = ${n}`);
  }
  L.push("    }");
  L.push(
    "    public struct TextStyle: Sendable { public let weight: Int; public let size: CGFloat; public let line: CGFloat }",
  );
  L.push("    public enum Typography {");
  for (const [k, s] of Object.entries(t.type))
    L.push(
      `        public static let ${camel(k)} = TextStyle(weight: ${s.weight}, size: ${s.size}, line: ${s.line})`,
    );
  L.push("    }");
  for (const th of THEMES) {
    L.push(`    public enum ${cap(th)} {`);
    for (const [k, v] of Object.entries(t.neutral[th])) {
      const c = rgba(v);
      if (c) L.push(`        public static let ${camel(k)} = ${swiftColor(c)}`);
    }
    L.push("    }");
  }
  // `primary*` are neutral theme tokens since 2.1 (D-155): the accent only carries fill/text/on.
  L.push(
    "    public struct Accent: Sendable { public let fill, text, on, primary, primaryText: Color }",
  );
  L.push("    public static let accents: [String: (dark: Accent, light: Accent)] = [");
  for (const [slug, th] of Object.entries(t.accent)) {
    const a = (theme) => {
      const v = th[theme];
      const n = t.neutral[theme];
      return `Accent(fill: ${swiftColor(rgba(v["accent-fill"]))}, text: ${swiftColor(rgba(v["accent-text"]))}, on: ${swiftColor(rgba(v["accent-on"]))}, primary: ${swiftColor(rgba(n.primary))}, primaryText: ${swiftColor(rgba(n["primary-text"]))})`;
    };
    L.push(`        "${slug}": (dark: ${a("dark")}, light: ${a("light")}),`);
  }
  L.push("    ]", "}", "");
  return L.join("\n");
}

function kColor(c) {
  const h = (x) => x.toString(16).padStart(2, "0").toUpperCase();
  return `Color(0x${h(Math.round(c[3] * 255))}${h(c[0])}${h(c[1])}${h(c[2])})`;
}
function genKotlin(t) {
  const L = [
    HEADER,
    "package studio.serkov.sirtone.mobile.design",
    "",
    "import androidx.compose.ui.graphics.Color",
    "import androidx.compose.ui.unit.dp",
    "import androidx.compose.ui.unit.sp",
    "",
  ];
  L.push("object SirToneMetric {");
  for (const [k, v] of Object.entries(t.base)) {
    const n = px(v);
    if (n !== null) L.push(`    val ${camel(k)} = ${n}.dp`);
    const d = ms(v);
    if (d !== null) L.push(`    const val ${camel(k)}Ms = ${d}`);
  }
  L.push(
    "}",
    "",
    "data class SirToneTextStyle(val weight: Int, val size: androidx.compose.ui.unit.TextUnit, val line: androidx.compose.ui.unit.TextUnit)",
    "",
    "object SirToneText {",
  );
  for (const [k, s] of Object.entries(t.type))
    L.push(`    val ${camel(k)} = SirToneTextStyle(${s.weight}, ${s.size}.sp, ${s.line}.sp)`);
  L.push("}", "");
  for (const th of THEMES) {
    L.push(`object SirTone${cap(th)} {`);
    for (const [k, v] of Object.entries(t.neutral[th])) {
      const c = rgba(v);
      if (c) L.push(`    val ${camel(k)} = ${kColor(c)}`);
    }
    L.push("}", "");
  }
  L.push(
    "data class SirToneAccent(val fill: Color, val text: Color, val on: Color, val primary: Color, val primaryText: Color)",
    "",
    "object SirToneAccents {",
  );
  L.push("    val all: Map<String, Pair<SirToneAccent, SirToneAccent>> = mapOf(");
  for (const [slug, th] of Object.entries(t.accent)) {
    const a = (theme) => {
      const v = th[theme];
      const n = t.neutral[theme];
      return `SirToneAccent(${kColor(rgba(v["accent-fill"]))}, ${kColor(rgba(v["accent-text"]))}, ${kColor(rgba(v["accent-on"]))}, ${kColor(rgba(n.primary))}, ${kColor(rgba(n["primary-text"]))})`;
    };
    L.push(`        "${slug}" to Pair(${a("dark")}, ${a("light")}),`);
  }
  L.push("    )", "}", "");
  return L.join("\n");
}

function write(path, text) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text.replace(/\r\n/g, "\n"));
}
const fileJson = (t) =>
  `${JSON.stringify(
    {
      $comment:
        "Machine copy of DESIGN.md `sirkov-tokens` (colours) and the desktop CSS (metrics, text styles), D-141/D-155. Regenerate only with scripts/tokens.mjs --extract.",
      ...t,
    },
    null,
    2,
  )}\n`;

const mode = process.argv[2] ?? "--check";
const fresh = structure();
if (mode === "--extract") {
  write(JSON_PATH, fileJson(fresh));
  console.log(
    `tokens.json: ${Object.keys(fresh.accent).length} accents, ${Object.keys(fresh.type).length} text styles`,
  );
} else {
  const stored = readFileSync(JSON_PATH, "utf8").replace(/\r\n/g, "\n");
  if (stored !== fileJson(fresh)) {
    console.error(
      "tokens.json differs from DESIGN.md / the desktop CSS — run scripts/tokens.mjs --extract",
    );
    process.exit(1);
  }
  const t = JSON.parse(stored);
  if (mode === "--gen") {
    write(SWIFT, genSwift(t));
    write(KOTLIN, genKotlin(t));
    console.log("tokens: Swift and Kotlin written");
  } else if (mode === "--check-gen") {
    const stale = [
      [SWIFT, genSwift(t)],
      [KOTLIN, genKotlin(t)],
    ].filter(([p, s]) => {
      try {
        return readFileSync(p, "utf8").replace(/\r\n/g, "\n") !== s;
      } catch {
        return true;
      }
    });
    if (stale.length) {
      console.error(
        `stale generated tokens: ${stale.map(([p]) => p).join(", ")} — run node scripts/tokens.mjs --gen`,
      );
      process.exit(1);
    }
    console.log("tokens: DESIGN.md, CSS, tokens.json, Swift and Kotlin agree");
  } else console.log("tokens: DESIGN.md, CSS and tokens.json agree");
}
