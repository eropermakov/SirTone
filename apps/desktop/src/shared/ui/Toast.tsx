import { CircleAlert, X } from "lucide-react";
import { useEffect } from "react";

type Props = {
  heading: string;
  message: string;
  closeLabel: string;
  action?: { label: string; onClick: () => void } | undefined;
  onClose: () => void;
  /** Auto-hide after this many ms (0 = stay). */
  timeoutMs?: number;
};

/**
 * Error toast per DESIGN.md §9: bottom right, `bg-flyout`, `danger` icon, optional «Повторить».
 * Modal error dialogs are forbidden. Announced politely to screen readers.
 */
export function Toast({ heading, message, closeLabel, action, onClose, timeoutMs = 6000 }: Props) {
  useEffect(() => {
    if (!timeoutMs) return;
    const id = window.setTimeout(onClose, timeoutMs);
    return () => window.clearTimeout(id);
  }, [onClose, timeoutMs]);

  return (
    <div
      role="status"
      aria-live="polite"
      className="fixed right-4 bottom-[calc(var(--player-height)+1rem)] z-50 flex w-80 items-start gap-3 rounded-(--radius-md) border border-border bg-bg-flyout p-4 shadow-(--shadow-flyout)"
    >
      <CircleAlert size={20} strokeWidth={1.5} className="shrink-0 text-danger" />
      <div className="min-w-0 flex-1">
        <div className="t-body-strong text-text-primary">{heading}</div>
        <div className="t-caption break-words text-text-secondary">{message}</div>
        {action && (
          <button
            type="button"
            onClick={action.onClick}
            className="t-body-strong mt-2 text-accent-text"
          >
            {action.label}
          </button>
        )}
      </div>
      <button
        type="button"
        aria-label={closeLabel}
        onClick={onClose}
        className="flex size-6 shrink-0 items-center justify-center rounded-full text-icon"
      >
        <X size={16} strokeWidth={1.5} />
      </button>
    </div>
  );
}
