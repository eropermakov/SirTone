import { Check, Tag } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { setTag, trackTags } from "@/shared/ipc/feedback";
import type { MenuItem } from "@/shared/ui/Menu";
import { notify } from "@/shared/ui/notify";

const TAGS = [
  "season_summer",
  "season_autumn",
  "season_winter",
  "season_spring",
  "no_season",
  "sleep",
  "focus",
  "for_all_hit",
  "not_for_company",
] as const;

/**
 * «Метки ›» (FR-LEARN-013, `14` §17): manual tags override the automatic estimates; a check
 * marks the set ones. Optimistic (DESIGN.md §1); «no_season» and the seasons exclude each other.
 */
export function useTrackTags(track: number | null): MenuItem {
  const { t } = useTranslation("shell");
  const [tags, setTags] = useState<string[]>([]);
  useEffect(() => {
    setTags([]);
    if (track == null) return;
    trackTags(track)
      .then(setTags)
      .catch(() => {});
  }, [track]);
  const toggle = (tag: string) => {
    if (track == null) return;
    const on = !tags.includes(tag);
    const prev = tags;
    let next = on ? [...tags, tag] : tags.filter((x) => x !== tag);
    if (on && tag === "no_season") next = next.filter((x) => !x.startsWith("season_"));
    if (on && tag.startsWith("season_")) next = next.filter((x) => x !== "no_season");
    setTags(next);
    setTag(track, tag, on).catch(() => {
      setTags(prev);
      notify(t("wave.failed"));
    });
  };
  return {
    id: "tags",
    label: t("tags.menu"),
    icon: Tag,
    disabled: track == null,
    submenu: TAGS.map((tag) => ({
      id: tag,
      label: t(`tags.${tag}`),
      icon: tags.includes(tag) ? Check : undefined,
      onSelect: () => toggle(tag),
    })),
  };
}
