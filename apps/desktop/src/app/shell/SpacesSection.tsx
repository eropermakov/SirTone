import { FolderPlus, Plus } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { pickFolder } from "@/shared/ipc/library";
import { createSpace, listSpaces, selectSpace, type SpacesState } from "@/shared/ipc/spaces";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";

export function SpacesSection({ onChanged }: { onChanged: () => void }) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [state, setState] = useState<SpacesState | null>(null);
  const [newOpen, setNewOpen] = useState(false);
  const [name, setName] = useState("");
  const [root, setRoot] = useState("");
  const [busy, setBusy] = useState(false);

  const refresh = () => listSpaces().then(setState).catch(() => {});
  useEffect(() => {
    void refresh();
  }, []);

  const choose = async (id: string) => {
    setBusy(true);
    try {
      setState(await selectSpace(id));
      onChanged();
    } finally {
      setBusy(false);
    }
  };
  const create = async () => {
    if (!name.trim() || !root) return;
    setBusy(true);
    try {
      setState(await createSpace(name, root));
      setNewOpen(false);
      setName("");
      setRoot("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between gap-4">
        <h2 className="t-shelf-title text-primary-text">{t("spaces.title")}</h2>
        <Button variant="secondary" onClick={() => setNewOpen(true)}>
          <Plus size={16} strokeWidth={1.5} /> {t("spaces.add")}
        </Button>
      </div>
      <div className="flex flex-col gap-2 rounded-(--radius-xl) border border-border bg-bg-raised p-3">
        {state?.spaces.map((space) => (
          <button
            key={space.id}
            type="button"
            disabled={busy || space.id === state.activeId}
            onClick={() => choose(space.id)}
            className="micro-press flex min-h-(--row-height) items-center justify-between gap-3 rounded-(--radius-md) px-3 text-left hover:bg-bg-hover disabled:cursor-default disabled:bg-selected"
          >
            <span className="min-w-0">
              <span className="t-body-strong block truncate text-text-primary">{space.name}</span>
              <span className="t-caption block truncate text-text-tertiary">{space.root}</span>
            </span>
            {space.id === state.activeId && <span className="t-badge text-accent-text">{t("spaces.active")}</span>}
          </button>
        ))}
      </div>
      <p className="t-caption text-text-secondary">{t("spaces.hint")}</p>
      {newOpen && <Dialog title={t("spaces.newTitle")} closeLabel={tc("errors.close")} onClose={() => setNewOpen(false)}>
        <div className="flex flex-col gap-4">
          <label className="flex flex-col gap-2">
            <span className="t-caption text-text-secondary">{t("spaces.name")}</span>
            <input value={name} onChange={(e) => setName(e.target.value)} className="h-(--button-height) rounded-(--radius-md) border border-border bg-bg-layer px-3 text-text-primary" />
          </label>
          <div className="flex items-center gap-3">
            <span className="t-caption min-w-0 flex-1 truncate text-text-secondary">{root || t("spaces.folder")}</span>
            <Button variant="secondary" onClick={async () => { const p = await pickFolder(); if (p) setRoot(p); }}><FolderPlus size={16} strokeWidth={1.5} /> {t("spaces.choose")}</Button>
          </div>
          <Button disabled={busy || !name.trim() || !root} onClick={create}>{t("spaces.create")}</Button>
        </div>
      </Dialog>}
    </div>
  );
}
