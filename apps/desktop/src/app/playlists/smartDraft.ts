/**
 * Smart playlist rules ↔ the editor's draft (T3.13; `05` §8.1 JSON). Fields and operators mirror
 * the whitelist in `sirtone-library/src/smart.rs` — Rust validates everything again. Pure (no
 * IPC), tested in `smartDraft.test.ts`.
 */

export type Kind = "text" | "num" | "date" | "bool" | "enum";

/** Fields the editor offers (Rust whitelist). `analysis` — empty until the sound analysis. */
export const FIELDS: { name: string; kind: Kind; values?: string[]; analysis?: boolean }[] = [
  { name: "title", kind: "text" },
  { name: "artist", kind: "text" },
  { name: "album", kind: "text" },
  { name: "genre", kind: "text" },
  { name: "year", kind: "num" },
  { name: "added_at", kind: "date" },
  { name: "last_played", kind: "date" },
  { name: "play_count", kind: "num" },
  { name: "skip_count", kind: "num" },
  { name: "liked", kind: "bool" },
  { name: "season_tag", kind: "enum", values: ["winter", "spring", "summer", "autumn", "none"] },
  { name: "duration", kind: "num" },
  { name: "format", kind: "text" },
  { name: "lossless", kind: "bool" },
  { name: "bitrate", kind: "num" },
  { name: "size", kind: "num" },
  { name: "path", kind: "text" },
  { name: "bpm", kind: "num", analysis: true },
  { name: "energy", kind: "num", analysis: true },
  { name: "valence", kind: "num", analysis: true },
  { name: "danceability", kind: "num", analysis: true },
  { name: "acousticness", kind: "num", analysis: true },
  {
    name: "mood",
    kind: "enum",
    values: ["happy", "upbeat", "calm", "sad", "positive", "negative"],
    analysis: true,
  },
  {
    name: "activity",
    kind: "enum",
    values: ["workout", "sleep", "work", "road", "rest", "party"],
    analysis: true,
  },
  { name: "lang", kind: "enum", values: ["ru", "foreign", "none", "unknown"], analysis: true },
];

export const OPS: Record<Kind, string[]> = {
  text: ["contains", "notContains", "is", "isNot", "startsWith", "endsWith"],
  num: ["gt", "lt", "is", "isNot", "inTheRange"],
  date: ["inTheLast", "notInTheLast", "before", "after"],
  bool: ["is"],
  enum: ["is", "isNot"],
};

export const SORTABLE = FIELDS.filter((f) => !["mood", "activity", "season_tag"].includes(f.name));

export type Value = string | boolean | [string, string];
export type Cond = { type: "cond"; field: string; op: string; value: Value };
export type Group = { type: "group"; all: boolean; items: Node[] };
/** A condition the editor cannot show (made over IPC): kept as it is. */
export type Raw = { type: "raw"; json: unknown };
export type Node = Cond | Group | Raw;

export type SmartDraft = {
  root: Group;
  sort: string;
  desc: boolean;
  limitKind: "" | "limit" | "limitPercent" | "limitMinutes" | "limitMb";
  limit: string;
  refresh: string;
};

type FieldDef = (typeof FIELDS)[number];
const TITLE: FieldDef = { name: "title", kind: "text" };
export const fieldOf = (name: string): FieldDef => FIELDS.find((f) => f.name === name) ?? TITLE;

export const blankValue = (kind: Kind, op: string, values?: string[]): Value => {
  if (op === "inTheRange") return ["", ""];
  if (kind === "bool") return true;
  if (kind === "enum") return values?.[0] ?? "";
  if (op === "inTheLast" || op === "notInTheLast") return "30";
  return "";
};

export const newCond = (): Cond => ({ type: "cond", field: "title", op: "contains", value: "" });

export const blankDraft = (): SmartDraft => ({
  root: { type: "group", all: true, items: [newCond()] },
  sort: "",
  desc: false,
  limitKind: "",
  limit: "",
  refresh: "live",
});

// ---------- JSON (`05` §8.1) ----------

/** A number from a field; an empty field stays "" so Rust refuses it («Проверьте условия»). */
const num = (s: string): number | string => (s.trim() === "" ? "" : Number(s));

const condJson = (c: Cond): unknown => {
  const f = fieldOf(c.field);
  let v: unknown = c.value;
  if (c.op === "inTheRange" && Array.isArray(c.value)) v = c.value.map(num);
  else if (f.kind === "num" || c.op === "inTheLast" || c.op === "notInTheLast")
    v = num(String(c.value));
  return { [c.op]: { [c.field]: v } };
};

const groupJson = (g: Group): unknown => ({
  [g.all ? "all" : "any"]: g.items.map((n) =>
    n.type === "group" ? groupJson(n) : n.type === "raw" ? n.json : condJson(n),
  ),
});

export function draftToJson(d: SmartDraft): string {
  const o = groupJson(d.root) as Record<string, unknown>;
  if (d.sort) o.sort = `${d.desc ? "-" : "+"}${d.sort}`;
  if (d.limitKind && Number(d.limit) > 0) o[d.limitKind] = Math.round(Number(d.limit));
  o.refresh = d.refresh;
  return JSON.stringify(o);
}

const nodeFrom = (j: Record<string, unknown>): Node | null => {
  const [key, body] = Object.entries(j)[0] ?? [];
  if (!key) return null;
  if (key === "all" || key === "any") {
    const items = (body as Record<string, unknown>[]).map(nodeFrom).filter((n): n is Node => !!n);
    return { type: "group", all: key === "all", items };
  }
  const [field, v] = Object.entries((body ?? {}) as Record<string, unknown>)[0] ?? [];
  const f = FIELDS.find((x) => x.name === field);
  // Not a field/operator pair the editor offers (inPlaylist, isMissing, key…): kept as is.
  if (!field || !f || !OPS[f.kind].includes(key)) return { type: "raw", json: j };
  const value: Value = Array.isArray(v)
    ? [String(v[0]), String(v[1])]
    : typeof v === "boolean"
      ? v
      : String(v);
  return { type: "cond", field, op: key, value };
};

/** Canonical rules JSON (from Rust) → a draft; unknown parts are dropped. */
export function jsonToDraft(text: string): SmartDraft {
  const d = blankDraft();
  try {
    const j = JSON.parse(text) as Record<string, unknown>;
    const rootKey = "all" in j ? "all" : "any";
    const root = nodeFrom({ [rootKey]: j[rootKey] ?? [] });
    if (root?.type === "group") d.root = root;
    const s = typeof j.sort === "string" ? (j.sort.split(",")[0] ?? "") : "";
    if (s && s !== "random") {
      d.desc = s.startsWith("-");
      d.sort = s.replace(/^[+-]/, "");
    }
    for (const k of ["limit", "limitPercent", "limitMinutes", "limitMb"] as const) {
      if (typeof j[k] === "number") {
        d.limitKind = k;
        d.limit = String(j[k]);
      }
    }
    if (typeof j.refresh === "string") d.refresh = j.refresh;
  } catch {
    // A broken draft starts over.
  }
  return d;
}
