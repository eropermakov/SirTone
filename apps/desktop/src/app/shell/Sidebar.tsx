import {
  CircleHelp,
  Disc3,
  ListMusic,
  Mic2,
  PanelLeftClose,
  PanelLeftOpen,
  Search,
  Settings,
  Waves,
} from "lucide-react";
import type { ComponentType } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/shared/lib/utils";

export type Screen =
  | "search"
  | "wave"
  | "artists"
  | "albums"
  | "playlists"
  | "unknown"
  | "settings";

type Item = { id: Screen; icon: ComponentType<{ size?: number; strokeWidth?: number }> };

/** Sidebar items — composition from the TZ (FR-SHELL-002), look from DESIGN.md §9 (Q-20). */
const TOP: Item[] = [{ id: "wave", icon: Waves }];
const COLLECTION: Item[] = [
  { id: "artists", icon: Mic2 },
  { id: "albums", icon: Disc3 },
];
const SETTINGS: Item = { id: "settings", icon: Settings };
const REST: Item[] = [
  { id: "playlists", icon: ListMusic },
  { id: "unknown", icon: CircleHelp },
];

type Props = {
  current: Screen;
  collapsed: boolean;
  onNavigate: (s: Screen) => void;
  onToggle: () => void;
};

export function Sidebar({ current, collapsed, onNavigate, onToggle }: Props) {
  const { t } = useTranslation("nav");
  const row = (it: Item) => {
    const Icon = it.icon;
    const active = current === it.id;
    return (
      <button
        key={it.id}
        type="button"
        onClick={() => onNavigate(it.id)}
        aria-current={active ? "page" : undefined}
        title={collapsed ? t(it.id) : undefined}
        className={cn(
          "flex h-(--item-height) w-full items-center gap-3 rounded-(--radius-md) px-3 text-left",
          "transition-colors duration-(--dur-fast)",
          active
            ? "bg-primary text-on-primary"
            : "text-text-primary hover:bg-bg-hover [&_svg]:text-icon",
          collapsed && "justify-center px-0",
        )}
      >
        <Icon size={20} strokeWidth={1.5} />
        {!collapsed && <span className="t-body truncate">{t(it.id)}</span>}
      </button>
    );
  };

  return (
    <nav
      aria-label={t("collection")}
      className={cn(
        "flex h-full shrink-0 flex-col gap-1 bg-bg-base p-3",
        collapsed ? "w-16" : "w-(--sidebar-width)",
      )}
    >
      <div
        className={cn("mb-2 flex items-center", collapsed ? "justify-center" : "justify-between")}
      >
        {!collapsed && (
          <span className="t-shelf-title px-2 text-text-primary">{t("common:app.name")}</span>
        )}
        <div className="flex">
          <button
            type="button"
            onClick={() => onNavigate("search")}
            aria-label={t("search")}
            className="flex size-9 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
          >
            <Search size={20} strokeWidth={1.5} />
          </button>
          {!collapsed && (
            <button
              type="button"
              onClick={onToggle}
              aria-label={t("collapse")}
              className="flex size-9 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
            >
              <PanelLeftClose size={20} strokeWidth={1.5} />
            </button>
          )}
        </div>
      </div>
      {TOP.map(row)}
      {!collapsed && (
        <div className="t-group-label mt-4 mb-1 px-3 text-text-secondary">{t("collection")}</div>
      )}
      {COLLECTION.map(row)}
      <div className="mt-4" />
      {REST.map(row)}
      <div className="flex-1" />
      {collapsed && (
        <button
          type="button"
          onClick={onToggle}
          aria-label={t("expand")}
          className="flex h-(--item-height) items-center justify-center rounded-(--radius-md) text-icon hover:bg-bg-hover"
        >
          <PanelLeftOpen size={20} strokeWidth={1.5} />
        </button>
      )}
      {row(SETTINGS)}
    </nav>
  );
}
