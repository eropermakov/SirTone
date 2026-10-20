import { Eye, Heart } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { getSettings, patchSettings } from "@/shared/ipc/settings";
import { cn } from "@/shared/lib/utils";
import { Dialog } from "@/shared/ui/Dialog";
import { Switch } from "@/shared/ui/Switch";
import { milkdropControl, useMilkdropControl } from "./milkdropControl";

/** How many rows are drawn at once (500 presets; the search narrows them). */
const SHOWN = 120;

/**
 * «Мои пресеты» (owner, 2026-10-20, D-105): every MilkDrop preset with ♥ and «показать сейчас»,
 * a search, and «Показывать только мои» — then only the ♥ ones take turns on screen. Temporary
 * look until the owner's design.
 */
export function PresetsDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const md = useMilkdropControl();
  const [list, setList] = useState(() => milkdropControl.list());
  const [q, setQ] = useState("");
  const [only, setOnly] = useState(false);
  useEffect(() => {
    getSettings().then((s) => {
      const v = s.viz as { milkdrop_favorites_only?: boolean } | undefined;
      setOnly(v?.milkdrop_favorites_only ?? false);
    });
  }, []);
  const mine = list.filter((p) => p.rating > 0).length;
  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase();
    return list
      .filter((p) => p.rating >= 0 && (!needle || p.name.toLowerCase().includes(needle)))
      .sort((a, b) => b.rating - a.rating);
  }, [list, q]);
  const like = (name: string, on: boolean) => {
    milkdropControl.rateName(name, on ? 1 : 0);
    setList((l) => l.map((p) => (p.name === name ? { ...p, rating: on ? 1 : 0 } : p)));
  };
  return (
    <Dialog title={t("viz.presets.title")} closeLabel={tc("errors.close")} onClose={onClose} wide>
      <div className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <div className="t-body text-text-primary">{t("viz.presets.onlyMine")}</div>
          <div className="t-caption text-text-secondary">
            {t("viz.presets.onlyMineHint")} {t("viz.presets.mine", { count: mine })}
          </div>
        </div>
        <Switch
          checked={only}
          label={t("viz.presets.onlyMine")}
          onChange={(v) => {
            setOnly(v);
            patchSettings({ viz: { milkdrop_favorites_only: v } }).catch(() => setOnly(!v));
          }}
        />
      </div>
      <input
        type="search"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder={t("viz.presets.search")}
        aria-label={t("viz.presets.search")}
        className="t-body h-(--button-height) rounded-full border border-border bg-bg-layer px-4 text-text-primary"
      />
      <ul className="m-0 flex max-h-96 list-none flex-col overflow-auto p-0">
        {shown.slice(0, SHOWN).map((p) => (
          <li
            key={p.name}
            className={cn(
              "flex items-center gap-2 rounded-(--radius-md) px-2 py-1 hover:bg-bg-hover",
              md.preset === p.name && "bg-bg-selected",
            )}
          >
            <button
              type="button"
              aria-pressed={p.rating > 0}
              aria-label={t("viz.like")}
              title={t("viz.like")}
              onClick={() => like(p.name, p.rating <= 0)}
              className={cn(
                "flex size-8 shrink-0 items-center justify-center rounded-full",
                p.rating > 0 ? "text-accent-text" : "text-icon",
              )}
            >
              <Heart size={16} strokeWidth={1.5} fill={p.rating > 0 ? "currentColor" : "none"} />
            </button>
            <span className="t-caption min-w-0 flex-1 truncate text-text-primary" title={p.name}>
              {p.name}
            </span>
            <button
              type="button"
              aria-label={t("viz.presets.show")}
              title={t("viz.presets.show")}
              onClick={() => milkdropControl.show(p.name)}
              className="flex size-8 shrink-0 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
            >
              <Eye size={16} strokeWidth={1.5} />
            </button>
          </li>
        ))}
        {shown.length === 0 && (
          <li className="t-caption px-2 py-3 text-text-secondary">{t("viz.presets.nothing")}</li>
        )}
      </ul>
    </Dialog>
  );
}
