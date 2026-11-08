import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { type PhoneSelection, type PickItem, type PickLists, pickLists } from "@/shared/ipc/phones";
import { cn } from "@/shared/lib/utils";
import { Skeleton } from "@/shared/ui/Skeleton";

type Group = keyof PickLists;
const GROUPS: Group[] = ["artists", "albums", "genres", "playlists"];

const keyOf = (group: Group, item: PickItem): string | number =>
  group === "genres" ? (item.key ?? "") : item.id;

/** Whether `item` of `group` is in the choice. */
function has(sel: PhoneSelection, group: Group, item: PickItem): boolean {
  return group === "genres"
    ? sel.genres.includes(item.key ?? "")
    : (sel[group] as number[]).includes(item.id);
}

function toggle(sel: PhoneSelection, group: Group, item: PickItem): PhoneSelection {
  if (group === "genres") {
    const k = item.key ?? "";
    return {
      ...sel,
      genres: sel.genres.includes(k) ? sel.genres.filter((g) => g !== k) : [...sel.genres, k],
    };
  }
  const list = sel[group] as number[];
  return {
    ...sel,
    [group]: list.includes(item.id) ? list.filter((i) => i !== item.id) : [...list, item.id],
  };
}

/**
 * «Выбранная» музыка for the phone (D-152, as Finder's «выбранные исполнители, альбомы, жанры и
 * плейлисты», `01d` §G): four groups, a search, ticks. The lists come once per opening.
 */
export function MusicPicker({
  value,
  onChange,
}: {
  value: PhoneSelection;
  onChange: (s: PhoneSelection) => void;
}) {
  const { t } = useTranslation("shell");
  const [lists, setLists] = useState<PickLists | null>(null);
  const [failed, setFailed] = useState(false);
  const [group, setGroup] = useState<Group>("artists");
  const [query, setQuery] = useState("");
  useEffect(() => {
    let alive = true;
    pickLists()
      .then((l) => alive && setLists(l))
      .catch(() => alive && setFailed(true));
    return () => {
      alive = false;
    };
  }, []);
  const shown = useMemo(() => {
    const q = query.trim().toLocaleLowerCase("ru");
    const all = lists?.[group] ?? [];
    return q
      ? all.filter((i) => `${i.name} ${i.detail ?? ""}`.toLocaleLowerCase("ru").includes(q))
      : all;
  }, [lists, group, query]);

  return (
    <div className="flex flex-col gap-2">
      <div role="tablist" aria-label={t("phones.pick.title")} className="flex flex-wrap gap-2">
        {GROUPS.map((g) => {
          const n = g === "genres" ? value.genres.length : (value[g] as number[]).length;
          return (
            <button
              key={g}
              type="button"
              role="tab"
              aria-selected={group === g}
              onClick={() => setGroup(g)}
              className={cn(
                "t-menu flex h-(--chip-height) items-center rounded-full border px-3 focus-visible:outline-2 focus-visible:outline-focus-ring",
                group === g
                  ? "border-transparent bg-primary text-on-primary"
                  : "border-border text-text-primary hover:bg-bg-hover",
              )}
            >
              {n > 0
                ? t("phones.pick.groupCount", { name: t(`phones.pick.${g}`), count: n })
                : t(`phones.pick.${g}`)}
            </button>
          );
        })}
      </div>
      <input
        type="search"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        placeholder={t("phones.pick.search")}
        aria-label={t("phones.pick.search")}
        className="t-body h-(--button-height) rounded-(--radius-md) border border-border bg-bg-layer px-3 text-text-primary"
      />
      {failed ? (
        <p role="alert" className="t-caption text-text-secondary">
          {t("phones.errors.Internal")}
        </p>
      ) : lists === null ? (
        <Skeleton className="h-40 w-full rounded-(--radius-md)" />
      ) : shown.length === 0 ? (
        <p className="t-caption text-text-secondary">{t("phones.pick.empty")}</p>
      ) : (
        <ul
          role="tabpanel"
          aria-label={t(`phones.pick.${group}`)}
          className="flex max-h-60 flex-col overflow-y-auto rounded-(--radius-md) border border-border"
        >
          {shown.map((item) => (
            <li key={`${group}-${keyOf(group, item)}`}>
              <label className="flex cursor-pointer items-center gap-3 px-3 py-2 hover:bg-bg-hover has-focus-visible:outline-2 has-focus-visible:outline-focus-ring">
                <input
                  type="checkbox"
                  checked={has(value, group, item)}
                  onChange={() => onChange(toggle(value, group, item))}
                  className="size-4 accent-(--accent-fill)"
                />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="t-body truncate text-text-primary">{item.name}</span>
                  {item.detail && (
                    <span className="t-caption truncate text-text-secondary">{item.detail}</span>
                  )}
                </span>
                <span className="t-caption text-text-tertiary">
                  {t("phones.pick.tracks", { count: item.tracks })}
                </span>
              </label>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
