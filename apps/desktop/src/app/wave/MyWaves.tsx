import { Bookmark } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { WaveQueryIpc } from "@/shared/ipc/bindings";
import { getSettings, onSettingsChanged, patchSettings } from "@/shared/ipc/settings";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { Menu } from "@/shared/ui/Menu";
import { notify } from "@/shared/ui/notify";

/** One saved setup (settings `wave.mine`, ≤ 20). */
export type MyWave = { id: string; name: string; query: WaveQueryIpc };

export const MY_WAVES_MAX = 20;
export const NAME_MAX = 40;

const read = (s: Record<string, unknown>): MyWave[] => {
  const w = s.wave as { mine?: MyWave[] | null } | undefined;
  return Array.isArray(w?.mine) ? w.mine : [];
};

/** The saved waves, kept current with the settings. */
export function useMyWaves(): [MyWave[], (next: MyWave[]) => Promise<void>] {
  const [list, setList] = useState<MyWave[]>([]);
  useEffect(() => {
    let alive = true;
    let off: (() => void) | undefined;
    const load = () => getSettings().then((s) => alive && setList(read(s)));
    load();
    onSettingsChanged(load).then((u) => {
      if (alive) off = u;
      else u();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, []);
  const save = async (next: MyWave[]) => {
    const prev = list;
    setList(next); // optimistic (DESIGN.md §1)
    try {
      await patchSettings({ wave: { mine: next } });
    } catch (e) {
      setList(prev);
      throw e;
    }
  };
  return [list, save];
}

/** A name for a saved wave: «Сохранить как свою волну», «Переименовать». */
export function NameDialog({
  title,
  initial,
  onDone,
  onClose,
}: {
  title: string;
  initial: string;
  onDone: (name: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [name, setName] = useState(initial);
  const ok = name.trim().length > 0;
  return (
    <Dialog title={title} closeLabel={tc("errors.close")} onClose={onClose}>
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          if (ok) onDone(name.trim());
        }}
      >
        <label className="flex flex-col gap-1">
          <span className="t-caption text-text-secondary">{t("wave.mine.name")}</span>
          <input
            value={name}
            maxLength={NAME_MAX}
            onChange={(e) => setName(e.target.value)}
            className="t-body h-(--button-height) rounded-(--radius-md) border border-border bg-bg-layer px-3 text-text-primary"
          />
        </label>
        <div className="flex justify-end gap-3">
          <Button variant="ghost" onClick={onClose}>
            {t("wave.mine.cancel")}
          </Button>
          <Button type="submit" disabled={!ok}>
            {t("wave.mine.save")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

/**
 * «Мои волны» (T5.17, FR-WENG-008; `01c` W-02 — YouTube Music «сохранить радио»): the saved
 * setups as cards next to the built-in waves; a click plays it, right click or «…» —
 * «Переименовать», «Удалить».
 */
export function MyWaves({
  list,
  save,
  onPlay,
}: {
  list: MyWave[];
  save: (next: MyWave[]) => Promise<void>;
  onPlay: (w: MyWave) => void;
}) {
  const { t } = useTranslation("shell");
  const [menu, setMenu] = useState<{ w: MyWave; x: number; y: number } | null>(null);
  const [rename, setRename] = useState<MyWave | null>(null);
  const failed = () => notify(t("wave.mine.failed"));
  return (
    <>
      {list.map((w) => (
        <Button
          key={w.id}
          variant="secondary"
          onClick={() => onPlay(w)}
          onContextMenu={(e) => {
            e.preventDefault();
            setMenu({ w, x: e.clientX, y: e.clientY });
          }}
          className="relative"
        >
          <Bookmark size={16} strokeWidth={1.5} />
          {w.name}
        </Button>
      ))}
      {menu && (
        <Menu
          at={menu}
          label={menu.w.name}
          onClose={() => setMenu(null)}
          items={[
            { id: "play", label: t("wave.mine.play"), onSelect: () => onPlay(menu.w) },
            { id: "rename", label: t("wave.mine.rename"), onSelect: () => setRename(menu.w) },
            { separator: true },
            {
              id: "delete",
              label: t("wave.mine.delete"),
              danger: true,
              onSelect: () => {
                const gone = menu.w;
                save(list.filter((x) => x.id !== gone.id))
                  .then(() =>
                    notify(t("wave.mine.deleted", { name: gone.name }), {
                      label: t("wave.mine.undo"),
                      onClick: () => void save(list).catch(failed),
                    }),
                  )
                  .catch(failed);
              },
            },
          ]}
        />
      )}
      {rename && (
        <NameDialog
          title={t("wave.mine.rename")}
          initial={rename.name}
          onClose={() => setRename(null)}
          onDone={(name) => {
            setRename(null);
            save(list.map((x) => (x.id === rename.id ? { ...x, name } : x))).catch(failed);
          }}
        />
      )}
    </>
  );
}
