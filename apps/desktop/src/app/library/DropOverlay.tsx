import { FolderDown } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { onFileDrag } from "@/shared/ipc/window";
import { cn } from "@/shared/lib/utils";

/**
 * «Отпустите, чтобы добавить» (`06` «Импорт»; Apple Music / Spotify drop target, `01b`) while
 * files from Explorer hover over the window. DESIGN.md has no drop overlay: `artwork-scrim` +
 * a neutral Panel and an `accent-text` ring (temporary, `06` §12). Never takes the pointer — the
 * drop goes to the window as before.
 */
export function DropOverlay() {
  const { t } = useTranslation("shell");
  const [over, setOver] = useState(false);
  useEffect(() => {
    let off: (() => void) | undefined;
    let alive = true;
    onFileDrag(setOver).then((f) => {
      if (alive) off = f;
      else f();
    });
    return () => {
      alive = false;
      off?.();
    };
  }, []);
  return (
    <div
      aria-hidden={!over}
      className={cn(
        "pointer-events-none fixed inset-0 z-(--z-overlay) flex items-center justify-center bg-(--artwork-scrim) ring-2 ring-accent-text ring-inset",
        "transition-opacity duration-(--dur-fast)",
        over ? "opacity-100" : "opacity-0",
      )}
    >
      <div
        role="status"
        className="flex flex-col items-center gap-2 rounded-(--radius-xl) border border-border bg-bg-raised p-6"
      >
        <FolderDown size={32} strokeWidth={1.5} className="text-accent-text" />
        <span className="t-card-title text-text-primary">{t("drop.title")}</span>
        <span className="t-caption text-text-secondary">{t("drop.text")}</span>
      </div>
    </div>
  );
}
