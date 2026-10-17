import { X } from "lucide-react";
import { type ReactNode, useEffect, useRef } from "react";
import { Button } from "./Button";

/**
 * A neutral Panel over a scrim (DESIGN.md §9 Panel; confirmations are allowed, error dialogs are
 * not — errors are toasts). Esc and the cross close it; focus moves inside and returns.
 */
export function Dialog({
  title,
  closeLabel,
  onClose,
  children,
}: {
  title: string;
  closeLabel: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const back = document.activeElement as HTMLElement | null;
    ref.current?.querySelector<HTMLElement>("input, button")?.focus();
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("keydown", key, true);
      back?.focus();
    };
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-(--z-modal) flex items-center justify-center bg-(--artwork-scrim)">
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className="flyout-in flex max-h-[90vh] w-[520px] max-w-[92vw] flex-col gap-4 overflow-auto rounded-(--radius-xl) border border-border bg-bg-raised p-6 shadow-(--shadow-flyout)"
      >
        <div className="flex items-start justify-between gap-4">
          <h2 className="t-card-title min-w-0 text-text-primary">{title}</h2>
          <Button variant="icon" label={closeLabel} onClick={onClose}>
            <X size={20} strokeWidth={1.5} />
          </Button>
        </div>
        {children}
      </div>
    </div>
  );
}
