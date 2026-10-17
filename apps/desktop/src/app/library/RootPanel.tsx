import { FolderOpen } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { type LibraryStatus, pickFolder, setRoot } from "@/shared/ipc/library";

/**
 * «Папка для музыки» — step 4 of the first-run setup (FR-ONB-001, Stage 2) used on its own until
 * then: shown when music is added before a library folder exists. Neutral Panel (DESIGN.md §9).
 */
export function RootPanel({
  status,
  onDone,
}: {
  status: LibraryStatus;
  onDone: (s: LibraryStatus) => void;
}) {
  const { t } = useTranslation("shell");
  const [path, setPath] = useState(status.root ?? status.defaultRoot);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  const change = async () => {
    const picked = await pickFolder();
    if (picked) {
      setPath(picked);
      setFailed(false);
    }
  };
  const confirm = async () => {
    setBusy(true);
    try {
      onDone(await setRoot(path));
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      aria-labelledby="root-panel-title"
      className="mx-auto mt-6 flex max-w-xl flex-col gap-4 rounded-(--radius-xl) border border-border bg-bg-raised p-6"
    >
      <h2 id="root-panel-title" className="t-card-title text-text-primary">
        {t("root.title")}
      </h2>
      <p className="t-body text-text-secondary">{t("root.text")}</p>
      <div className="flex items-center gap-3 rounded-(--radius-md) bg-bg-layer px-3 py-2">
        <FolderOpen size={20} strokeWidth={1.5} className="shrink-0 text-icon" />
        <span className="t-body min-w-0 flex-1 truncate text-text-primary" title={path}>
          {path}
        </span>
        <button type="button" onClick={change} className="t-body-strong shrink-0 text-accent-text">
          {t("root.change")}
        </button>
      </div>
      {failed && <p className="t-caption text-danger">{t("root.error")}</p>}
      <div>
        <button
          type="button"
          disabled={busy || !path}
          onClick={confirm}
          className="t-body-strong flex h-(--button-height) items-center rounded-full bg-accent-fill px-5 text-accent-on disabled:opacity-40"
        >
          {t("root.continue")}
        </button>
      </div>
    </section>
  );
}
