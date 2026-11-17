import { ChevronDown, FolderCog, Pencil, Plus, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  createSpace,
  type LocalSpace,
  listSpaces,
  removeSpace,
  renameSpace,
  selectSpace,
} from "@/shared/ipc/spaces";
import { cn } from "@/shared/lib/utils";
import { notify } from "@/shared/ui/notify";

/** Compact local-space picker. Switching intentionally restarts Rust services (D-178). */
export function SpaceMenu({ collapsed }: { collapsed: boolean }) {
  const { t } = useTranslation("shell");
  const [spaces, setSpaces] = useState<LocalSpace[]>([]);
  const [open, setOpen] = useState(false);

  const reload = useCallback(
    () =>
      listSpaces()
        .then(setSpaces)
        .catch(() => notify(t("spaces.error"))),
    [t],
  );
  useEffect(() => {
    reload();
  }, [reload]);

  const active = spaces.find((space) => space.active);
  const askName = (title: string, value = "") => window.prompt(title, value)?.trim() ?? "";
  const create = async () => {
    const name = askName(t("spaces.createPrompt"));
    if (!name) return;
    try {
      const space = await createSpace(name);
      setSpaces((current) => [...current, space]);
    } catch {
      notify(t("spaces.error"));
    }
  };
  const rename = async (space: LocalSpace) => {
    const name = askName(t("spaces.renamePrompt"), space.name);
    if (!name || name === space.name) return;
    try {
      await renameSpace(space.id, name);
      setSpaces((current) =>
        current.map((item) => (item.id === space.id ? { ...item, name } : item)),
      );
    } catch {
      notify(t("spaces.error"));
    }
  };
  const remove = async (space: LocalSpace) => {
    if (!window.confirm(t("spaces.removeConfirm", { name: space.name }))) return;
    try {
      await removeSpace(space.id);
      setSpaces((current) => current.filter((item) => item.id !== space.id));
    } catch {
      notify(t("spaces.error"));
    }
  };

  return (
    <div className="relative mb-3 px-1">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        aria-label={t("spaces.switch")}
        title={collapsed ? (active?.name ?? t("spaces.title")) : undefined}
        className={cn(
          "flex h-11 w-full items-center gap-3 rounded-(--radius-sm) px-3 text-left text-text-secondary",
          "transition-colors duration-(--dur-fast) hover:bg-bg-hover hover:text-text-primary",
          collapsed && "justify-center px-0",
        )}
      >
        <FolderCog size={20} strokeWidth={1.5} />
        {!collapsed && (
          <span className="t-body min-w-0 flex-1 truncate">
            {active?.name || t("spaces.default")}
          </span>
        )}
        {!collapsed && (
          <ChevronDown
            size={16}
            className={cn("transition-transform duration-(--dur-fast)", open && "rotate-180")}
          />
        )}
      </button>
      {open && (
        <div className="glass absolute z-30 mt-1 flex w-[calc(var(--sidebar-width)-2rem)] flex-col gap-1 rounded-(--radius-md) p-2 shadow-lg">
          <span className="t-group-label px-2 py-1 text-text-secondary">{t("spaces.title")}</span>
          {spaces.map((space) => (
            <div
              key={space.id}
              className="flex h-10 items-center gap-1 rounded-(--radius-sm) px-2 hover:bg-bg-hover"
            >
              <button
                type="button"
                onClick={() => void selectSpace(space.id)}
                disabled={space.active}
                className={cn(
                  "t-body min-w-0 flex-1 truncate text-left",
                  space.active ? "text-text-primary" : "text-text-secondary",
                )}
              >
                {space.name || t("spaces.default")}
              </button>
              <button
                type="button"
                onClick={() => void rename(space)}
                aria-label={t("spaces.rename")}
                className="flex size-8 items-center justify-center text-icon hover:text-text-primary"
              >
                <Pencil size={16} strokeWidth={1.5} />
              </button>
              {!space.active && space.id !== "default" && (
                <button
                  type="button"
                  onClick={() => void remove(space)}
                  aria-label={t("spaces.remove")}
                  className="flex size-8 items-center justify-center text-icon hover:text-danger"
                >
                  <Trash2 size={16} strokeWidth={1.5} />
                </button>
              )}
            </div>
          ))}
          <button
            type="button"
            onClick={() => void create()}
            className="flex h-10 items-center gap-2 rounded-(--radius-sm) px-2 text-left text-text-primary hover:bg-bg-hover"
          >
            <Plus size={18} strokeWidth={1.5} />
            <span className="t-body">{t("spaces.create")}</span>
          </button>
        </div>
      )}
    </div>
  );
}
