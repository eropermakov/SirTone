import { useState, useSyncExternalStore } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/shared/lib/utils";
import { Dialog } from "@/shared/ui/Dialog";

/** The help's sections; the words are in i18n (`help.<id>.title` / `.body`, paragraphs by «\n»). */
const SECTIONS = ["music", "wave", "viz", "modes", "shell", "keys", "privacy", "portable"] as const;

let open = false;
const subs = new Set<() => void>();
const set = (v: boolean) => {
  open = v;
  for (const s of subs) s();
};

/** F1 and «О программе → Справка». */
export const openHelp = (): void => set(true);

/**
 * «Справка» (T7.8): a short guide inside the app — what Сиртон does and where it lives, one
 * section open at a time. Only features that exist; the first-run tips stay their own thing.
 */
export function HelpHost() {
  const shown = useSyncExternalStore(
    (cb) => {
      subs.add(cb);
      return () => subs.delete(cb);
    },
    () => open,
  );
  return shown ? <HelpDialog onClose={() => set(false)} /> : null;
}

function HelpDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [at, setAt] = useState<(typeof SECTIONS)[number]>("music");
  return (
    <Dialog title={t("help.title")} closeLabel={tc("errors.close")} onClose={onClose} wide>
      <div className="flex max-h-[65vh] gap-6" data-testid="help">
        <nav aria-label={t("help.title")} className="flex w-48 shrink-0 flex-col gap-1">
          {SECTIONS.map((id) => (
            <button
              key={id}
              type="button"
              aria-current={at === id ? "page" : undefined}
              onClick={() => setAt(id)}
              className={cn(
                "t-body rounded-(--radius-md) px-3 py-2 text-left",
                at === id
                  ? "bg-bg-selected text-text-primary"
                  : "text-text-secondary hover:bg-bg-hover",
              )}
            >
              {t(`help.${id}.title`)}
            </button>
          ))}
        </nav>
        <section className="flex min-w-0 flex-1 flex-col gap-3 overflow-auto pr-1">
          <h3 className="t-card-title text-text-primary">{t(`help.${at}.title`)}</h3>
          {t(`help.${at}.body`)
            .split("\n")
            .map((p) => (
              <p key={p} className="t-body text-text-secondary">
                {p}
              </p>
            ))}
        </section>
      </div>
    </Dialog>
  );
}
