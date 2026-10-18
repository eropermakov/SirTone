import { Minus, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { smartPreview } from "@/shared/ipc/playlists";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import {
  blankValue,
  type Cond,
  draftToJson,
  FIELDS,
  fieldOf,
  type Group,
  newCond,
  OPS,
  type Node as RuleNode,
  type SmartDraft,
  SORTABLE,
} from "./smartDraft";

export { blankDraft, jsonToDraft, type SmartDraft } from "./smartDraft";

/**
 * The smart playlist editor (T3.13, FR-PLS-002; Apple Music rules, Navidrome `.nsp` schema —
 * `05` §8.1, `01b` F-13): «все / любое» groups that nest, rows «поле · условие · значение · −»,
 * sort, limit and updating; «Подходит N треков» counts live. Fields and operators mirror the
 * whitelist in `sirtone-library/src/smart.rs` — Rust validates everything again.
 */

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
  const set = (i: number, n: RuleNode | null) =>
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
        n.type === "raw" ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: rows have no identity of their own
          <div key={i} className="flex items-center gap-2">
            <span className="t-body flex-1 text-text-secondary">{t("smart.raw")}</span>
            <Button variant="icon" label={t("smart.remove")} onClick={() => set(i, null)}>
              <Minus size={16} strokeWidth={1.5} />
            </Button>
          </div>
        ) : n.type === "group" ? (
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
