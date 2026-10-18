import { Minus, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { smartPreview } from "@/shared/ipc/playlists";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";

/**
 * The smart playlist editor (T3.13, FR-PLS-002; Apple Music rules, Navidrome `.nsp` schema —
 * `05` §8.1, `01b` F-13): «все / любое» groups that nest, rows «поле · условие · значение · −»,
 * sort, limit and updating; «Подходит N треков» counts live. Fields and operators mirror the
 * whitelist in `sirtone-library/src/smart.rs` — Rust validates everything again.
 */

type Kind = "text" | "num" | "date" | "bool" | "enum";

/** Fields the editor offers (Rust whitelist). `analysis` — empty until the sound analysis. */
const FIELDS: { name: string; kind: Kind; values?: string[]; analysis?: boolean }[] = [
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

const OPS: Record<Kind, string[]> = {
  text: ["contains", "notContains", "is", "isNot", "startsWith", "endsWith"],
  num: ["gt", "lt", "is", "isNot", "inTheRange"],
  date: ["inTheLast", "notInTheLast", "before", "after"],
  bool: ["is"],
  enum: ["is", "isNot"],
};

const SORTABLE = FIELDS.filter((f) => !["mood", "activity", "season_tag"].includes(f.name));

type Value = string | boolean | [string, string];
type Cond = { type: "cond"; field: string; op: string; value: Value };
type Group = { type: "group"; all: boolean; items: Node[] };
type Node = Cond | Group;

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
const fieldOf = (name: string): FieldDef => FIELDS.find((f) => f.name === name) ?? TITLE;

const blankValue = (kind: Kind, op: string, values?: string[]): Value => {
  if (op === "inTheRange") return ["", ""];
  if (kind === "bool") return true;
  if (kind === "enum") return values?.[0] ?? "";
  if (op === "inTheLast" || op === "notInTheLast") return "30";
  return "";
};

const newCond = (): Cond => ({ type: "cond", field: "title", op: "contains", value: "" });

export const blankDraft = (): SmartDraft => ({
  root: { type: "group", all: true, items: [newCond()] },
  sort: "",
  desc: false,
  limitKind: "",
  limit: "",
  refresh: "live",
});

// ---------- JSON (`05` §8.1) ----------

const condJson = (c: Cond): unknown => {
  const f = fieldOf(c.field);
  let v: unknown = c.value;
  if (c.op === "inTheRange" && Array.isArray(c.value)) v = c.value.map(Number);
  else if (f.kind === "num" || c.op === "inTheLast" || c.op === "notInTheLast") v = Number(c.value);
  return { [c.op]: { [c.field]: v } };
};

const groupJson = (g: Group): unknown => ({
  [g.all ? "all" : "any"]: g.items.map((n) => (n.type === "group" ? groupJson(n) : condJson(n))),
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
  const [field, v] = Object.entries(body as Record<string, unknown>)[0] ?? [];
  if (!field || !FIELDS.some((f) => f.name === field)) return null;
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

// ---------- UI ----------

const control =
  "t-body h-(--button-height) rounded-(--radius-md) border border-border bg-bg-layer px-2 text-text-primary";

function CondRow({
  c,
  onChange,
  onRemove,
}: {
  c: Cond;
  onChange: (c: Cond) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation("shell");
  const f = fieldOf(c.field);
  const setField = (name: string) => {
    const nf = fieldOf(name);
    const op = OPS[nf.kind][0] ?? "is";
    onChange({ type: "cond", field: name, op, value: blankValue(nf.kind, op, nf.values) });
  };
  const setOp = (op: string) =>
    onChange({
      ...c,
      op,
      value:
        op === "inTheRange" || c.op === "inTheRange" ? blankValue(f.kind, op, f.values) : c.value,
    });
  const v = c.value;
  return (
    <div className="flex flex-wrap items-center gap-2">
      <select
        aria-label={t("smart.field")}
        value={c.field}
        onChange={(e) => setField(e.target.value)}
        className={control}
      >
        {FIELDS.map((x) => (
          <option key={x.name} value={x.name}>
            {t(`smart.fields.${x.name}`)}
          </option>
        ))}
      </select>
      <select
        aria-label={t("smart.operator")}
        value={c.op}
        onChange={(e) => setOp(e.target.value)}
        className={control}
      >
        {OPS[f.kind].map((o) => (
          <option key={o} value={o}>
            {t(`smart.ops.${f.kind === "date" ? `date_${o}` : o}`)}
          </option>
        ))}
      </select>
      {f.kind === "enum" ? (
        <select
          aria-label={t("smart.value")}
          value={String(v)}
          onChange={(e) => onChange({ ...c, value: e.target.value })}
          className={control}
        >
          {f.values?.map((x) => (
            <option key={x} value={x}>
              {t(`smart.values.${f.name}.${x}`)}
            </option>
          ))}
        </select>
      ) : f.kind === "bool" ? (
        <select
          aria-label={t("smart.value")}
          value={v === true ? "1" : "0"}
          onChange={(e) => onChange({ ...c, value: e.target.value === "1" })}
          className={control}
        >
          <option value="1">{t("smart.yes")}</option>
          <option value="0">{t("smart.no")}</option>
        </select>
      ) : Array.isArray(v) ? (
        <>
          <input
            aria-label={t("smart.from")}
            type="number"
            value={v[0]}
            onChange={(e) => onChange({ ...c, value: [e.target.value, v[1]] })}
            className={`${control} w-24`}
          />
          <input
            aria-label={t("smart.to")}
            type="number"
            value={v[1]}
            onChange={(e) => onChange({ ...c, value: [v[0], e.target.value] })}
            className={`${control} w-24`}
          />
        </>
      ) : (
        <input
          aria-label={t("smart.value")}
          type={
            c.op === "before" || c.op === "after"
              ? "date"
              : f.kind === "num" || c.op === "inTheLast" || c.op === "notInTheLast"
                ? "number"
                : "text"
          }
          value={String(v)}
          onChange={(e) => onChange({ ...c, value: e.target.value })}
          className={`${control} min-w-0 flex-1`}
        />
      )}
      {(c.op === "inTheLast" || c.op === "notInTheLast") && (
        <span className="t-caption text-text-secondary">{t("smart.days")}</span>
      )}
      <Button variant="icon" label={t("smart.remove")} onClick={onRemove}>
        <Minus size={16} strokeWidth={1.5} />
      </Button>
      {f.analysis && (
        <p className="t-caption w-full text-text-secondary">{t("smart.needsAnalysis")}</p>
      )}
    </div>
  );
}

function GroupBox({
  g,
  depth,
  onChange,
  onRemove,
}: {
  g: Group;
  depth: number;
  onChange: (g: Group) => void;
  onRemove?: () => void;
}) {
  const { t } = useTranslation("shell");
  const set = (i: number, n: Node | null) =>
    onChange({
      ...g,
      items:
        n === null ? g.items.filter((_, k) => k !== i) : g.items.map((x, k) => (k === i ? n : x)),
    });
  return (
    <div
      className={
        depth > 0
          ? "flex flex-col gap-2 rounded-(--radius-md) border border-border p-3"
          : "flex flex-col gap-2"
      }
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className="t-body text-text-primary">{t("smart.match")}</span>
        <select
          aria-label={t("smart.match")}
          value={g.all ? "all" : "any"}
          onChange={(e) => onChange({ ...g, all: e.target.value === "all" })}
          className={control}
        >
          <option value="all">{t("smart.all")}</option>
          <option value="any">{t("smart.any")}</option>
        </select>
        <span className="t-body text-text-primary">{t("smart.conditions")}</span>
        <div className="flex-1" />
        {onRemove && (
          <Button variant="icon" label={t("smart.removeGroup")} onClick={onRemove}>
            <Minus size={16} strokeWidth={1.5} />
          </Button>
        )}
      </div>
      {g.items.map((n, i) =>
        n.type === "group" ? (
          <GroupBox
            // biome-ignore lint/suspicious/noArrayIndexKey: rows have no identity of their own
            key={i}
            g={n}
            depth={depth + 1}
            onChange={(x) => set(i, x)}
            onRemove={() => set(i, null)}
          />
        ) : (
          // biome-ignore lint/suspicious/noArrayIndexKey: rows have no identity of their own
          <CondRow key={i} c={n} onChange={(x) => set(i, x)} onRemove={() => set(i, null)} />
        ),
      )}
      <div className="flex flex-wrap gap-2">
        <Button variant="ghost" onClick={() => onChange({ ...g, items: [...g.items, newCond()] })}>
          <Plus size={16} strokeWidth={1.5} />
          {t("smart.addCondition")}
        </Button>
        {depth < 4 && (
          <Button
            variant="ghost"
            onClick={() =>
              onChange({
                ...g,
                items: [...g.items, { type: "group", all: false, items: [newCond()] }],
              })
            }
          >
            <Plus size={16} strokeWidth={1.5} />
            {t("smart.addGroup")}
          </Button>
        )}
      </div>
    </div>
  );
}

export function SmartEditor({
  initial,
  name,
  onName,
  onSave,
  onClose,
}: {
  initial: SmartDraft;
  /** Shown for a new playlist (the name field). */
  name?: string;
  onName?: (n: string) => void;
  onSave: (rulesJson: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [d, setD] = useState<SmartDraft>(initial);
  const [count, setCount] = useState<number | null>(null);
  const [bad, setBad] = useState(false);
  const json = draftToJson(d);

  // «Подходит N треков» — debounced; bad rules say so instead of a number.
  useEffect(() => {
    const id = window.setTimeout(() => {
      smartPreview(json)
        .then((n) => {
          setCount(n);
          setBad(false);
        })
        .catch(() => setBad(true));
    }, 300);
    return () => window.clearTimeout(id);
  }, [json]);

  return (
    <Dialog title={t("smart.title")} closeLabel={tc("errors.close")} onClose={onClose} wide>
      {onName && (
        <input
          aria-label={t("playlists.name")}
          value={name ?? ""}
          onChange={(e) => onName(e.target.value)}
          className={`${control} t-body-strong`}
        />
      )}
      <GroupBox g={d.root} depth={0} onChange={(root) => setD({ ...d, root })} />
      <div className="flex flex-wrap items-center gap-2">
        <span className="t-body text-text-primary">{t("smart.sort")}</span>
        <select
          aria-label={t("smart.sort")}
          value={d.sort}
          onChange={(e) => setD({ ...d, sort: e.target.value })}
          className={control}
        >
          <option value="">{t("smart.sortDefault")}</option>
          {SORTABLE.map((f) => (
            <option key={f.name} value={f.name}>
              {t(`smart.fields.${f.name}`)}
            </option>
          ))}
        </select>
        {d.sort && (
          <select
            aria-label={t("smart.direction")}
            value={d.desc ? "desc" : "asc"}
            onChange={(e) => setD({ ...d, desc: e.target.value === "desc" })}
            className={control}
          >
            <option value="asc">{t("smart.asc")}</option>
            <option value="desc">{t("smart.desc")}</option>
          </select>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <span className="t-body text-text-primary">{t("smart.limit")}</span>
        <select
          aria-label={t("smart.limit")}
          value={d.limitKind}
          onChange={(e) => setD({ ...d, limitKind: e.target.value as SmartDraft["limitKind"] })}
          className={control}
        >
          <option value="">{t("smart.limits.none")}</option>
          <option value="limit">{t("smart.limits.limit")}</option>
          <option value="limitPercent">{t("smart.limits.limitPercent")}</option>
          <option value="limitMinutes">{t("smart.limits.limitMinutes")}</option>
          <option value="limitMb">{t("smart.limits.limitMb")}</option>
        </select>
        {d.limitKind && (
          <input
            aria-label={t("smart.limitValue")}
            type="number"
            min={1}
            value={d.limit}
            onChange={(e) => setD({ ...d, limit: e.target.value })}
            className={`${control} w-24`}
          />
        )}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <span className="t-body text-text-primary">{t("smart.refresh")}</span>
        <select
          aria-label={t("smart.refresh")}
          value={d.refresh}
          onChange={(e) => setD({ ...d, refresh: e.target.value })}
          className={control}
        >
          <option value="live">{t("smart.refreshes.live")}</option>
          <option value="12h">{t("smart.refreshes.12h")}</option>
          <option value="1d">{t("smart.refreshes.1d")}</option>
          <option value="manual">{t("smart.refreshes.manual")}</option>
        </select>
      </div>
      <p role="status" className="t-caption min-h-5 text-text-secondary">
        {bad ? t("smart.invalid") : count === null ? null : t("smart.matches", { count })}
      </p>
      <div className="flex flex-wrap gap-3">
        <Button variant="primary" disabled={bad} onClick={() => onSave(json)}>
          {t("smart.save")}
        </Button>
        <Button variant="ghost" onClick={onClose}>
          {t("trackCard.cancel")}
        </Button>
      </div>
    </Dialog>
  );
}
