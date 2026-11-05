#!/usr/bin/env node
// Design tokens (D-141, M1.2): `design/tokens.json` is the machine copy of `DESIGN.md` §13 as the
// desktop app uses it (`apps/desktop/src/shared/ui/tokens/sirtone.css`). This script
//   --extract  builds tokens.json from the CSS (one-off, and after an owner-approved CSS change);
//   --check    fails if tokens.json and the CSS disagree (gate: the desktop look never drifts);
//   --gen      writes the Swift and Kotlin token files for the phones.
// Colours are never copied by hand.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const CSS = join(ROOT, "apps/desktop/src/shared/ui/tokens/sirtone.css");
const JSON_PATH = join(ROOT, "design/tokens.json");
const SWIFT = join(ROOT, "apps/mobile-ios/Sources/SirToneDesign/Tokens.swift");
const KOTLIN = join(ROOT, "apps/mobile-android/design/Tokens.kt");

/** CSS → list of { selector, vars } blocks (comments dropped, selectors normalised). */
function parseCss(text) {
  const src = text.replace(/\/\*[\s\S]*?\*\//g, "");
  const blocks = [];
  const re = /([^{}]+)\{([^{}]*)\}/g;
  for (const m of src.matchAll(re)) {
    const selector = m[1].replace(/\s+/g, " ").trim();
    const body = m[2];
    const vars = {};
    const props = {};
    for (const decl of body.split(";")) {
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

const DARK = ':root, [data-theme="dark"]';
const LIGHT = '[data-theme="light"]';
const accentSel = (slug, theme) =>
  theme === "dark"
    ? `[data-theme="dark"][data-accent="${slug}"], :root:not([data-theme])[data-accent="${slug}"]`
    : `[data-theme="light"][data-accent="${slug}"]`;

/** Blocks of the CSS in the structured form of tokens.json. */
function structure(blocks) {
  const t = { base: {}, neutral: {}, accent: {}, palette: {}, type: {} };
  let neutralDark = 0;
  let neutralLight = 0;
  for (const b of blocks) {
    const acc = b.selector.match(/\[data-accent="([a-z]+)"\]/);
    if (b.selector === ":root") Object.assign(t.base, b.vars);
    else if (acc) {
      const theme = b.selector.startsWith(LIGHT) ? "light" : "dark";
      if (!t.accent[acc[1]]) t.accent[acc[1]] = {};
      t.accent[acc[1]][theme] = b.vars;
    } else if (b.selector === DARK) {
      if (neutralDark++ === 0) t.neutral.dark = b.vars;
      else t.palette.dark = b.vars;
    } else if (b.selector === LIGHT) {
      if (neutralLight++ === 0) t.neutral.light = b.vars;
      else t.palette.light = b.vars;
    } else if (/^\.t-[a-z-]+$/.test(b.selector) && b.props.font) {
      const f = b.props.font.match(/^(\d+) (\d+)px \/ (\d+)px/);
      if (f) t.type[b.selector.slice(3)] = { weight: +f[1], size: +f[2], line: +f[3] };
    }
  }
  return t;
}

/** tokens.json → the CSS blocks it stands for, to compare with the real CSS. */
function expectedBlocks(t) {
  const out = [{ selector: ":root", vars: t.base }];
  out.push({ selector: DARK, vars: t.neutral.dark }, { selector: LIGHT, vars: t.neutral.light });
  for (const [slug, themes] of Object.entries(t.accent)) {
    for (const [theme, vars] of Object.entries(themes))
      out.push({ selector: accentSel(slug, theme), vars });
  }
  out.push({ selector: DARK, vars: t.palette.dark }, { selector: LIGHT, vars: t.palette.light });
  return out;
}

const key = (b) => `${b.selector} ${JSON.stringify(Object.entries(b.vars).sort())}`;

function check(t, blocks) {
  const errors = [];
  const real = blocks
    .filter((b) => Object.keys(b.vars).length > 0)
    .map(key)
    .sort();
  const want = expectedBlocks(t).map(key).sort();
  for (const k of want) if (!real.includes(k)) errors.push(`missing in CSS: ${k.slice(0, 160)}`);
  for (const k of real)
    if (!want.includes(k)) errors.push(`not in tokens.json: ${k.slice(0, 160)}`);
  const type = structure(blocks).type;
  if (JSON.stringify(type) !== JSON.stringify(t.type)) errors.push("text styles (.t-*) differ");
  return errors;
}

// ---- generation for the phones -------------------------------------------------------------

/** "#rrggbb" or "rgba(r, g, b, a)" → [r, g, b, a] (0…1), or null for non-colours. */
function rgba(v) {
  let m = v.match(/^#([0-9a-f]{6})$/i);
  if (m) {
    const n = parseInt(m[1], 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255, 1];
  }
  m = v.match(/^rgba\((\d+), (\d+), (\d+), ([\d.]+)\)$/);
  return m ? [+m[1], +m[2], +m[3], +m[4]] : null;
}
const camel = (s) => s.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase()).replace(/^(\d)/, "n$1");
const px = (v) => (/^\d+(\.\d+)?px$/.test(v) ? parseFloat(v) : null);
const ms = (v) => (/^\d+ms$/.test(v) ? parseInt(v, 10) : null);
const HEADER = "// Generated by scripts/tokens.mjs from design/tokens.json (D-141). Do not edit.\n";

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
  for (const theme of ["dark", "light"]) {
    L.push(`    public enum ${theme === "dark" ? "Dark" : "Light"} {`);
    for (const [k, v] of Object.entries(t.neutral[theme])) {
      const c = rgba(v);
      if (c) L.push(`        public static let ${camel(k)} = ${swiftColor(c)}`);
    }
    L.push("    }");
  }
  L.push(
    "    public struct Accent: Sendable { public let fill, text, on, primary, primaryText: Color }",
  );
  L.push("    public static let accents: [String: (dark: Accent, light: Accent)] = [");
  for (const [slug, th] of Object.entries(t.accent)) {
    const a = (v) =>
      `Accent(fill: ${swiftColor(rgba(v["accent-fill"]))}, text: ${swiftColor(rgba(v["accent-text"]))}, on: ${swiftColor(rgba(v["accent-on"]))}, primary: ${swiftColor(rgba(v.primary))}, primaryText: ${swiftColor(rgba(v["primary-text"]))})`;
    L.push(`        "${slug}": (dark: ${a(th.dark)}, light: ${a(th.light)}),`);
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
  for (const theme of ["dark", "light"]) {
    L.push(`object SirTone${theme === "dark" ? "Dark" : "Light"} {`);
    for (const [k, v] of Object.entries(t.neutral[theme])) {
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
    const a = (v) =>
      `SirToneAccent(${kColor(rgba(v["accent-fill"]))}, ${kColor(rgba(v["accent-text"]))}, ${kColor(rgba(v["accent-on"]))}, ${kColor(rgba(v.primary))}, ${kColor(rgba(v["primary-text"]))})`;
    L.push(`        "${slug}" to Pair(${a(th.dark)}, ${a(th.light)}),`);
  }
  L.push("    )", "}", "");
  return L.join("\n");
}

function write(path, text) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text.replace(/\r\n/g, "\n"));
}

const mode = process.argv[2] ?? "--check";
const blocks = parseCss(readFileSync(CSS, "utf8"));
if (mode === "--extract") {
  const t = structure(blocks);
  write(
    JSON_PATH,
    `${JSON.stringify({ $comment: "Machine copy of DESIGN.md §13 as used by the desktop CSS (D-141). Regenerate only with scripts/tokens.mjs --extract.", ...t }, null, 2)}\n`,
  );
  console.log(
    `tokens.json: ${Object.keys(t.accent).length} accents, ${Object.keys(t.type).length} text styles`,
  );
} else {
  const t = JSON.parse(readFileSync(JSON_PATH, "utf8"));
  const errors = check(t, blocks);
  if (errors.length) {
    console.error(errors.join("\n"));
    process.exit(1);
  }
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
    console.log("tokens: CSS, tokens.json, Swift and Kotlin agree");
  } else console.log("tokens: CSS and tokens.json agree");
}
