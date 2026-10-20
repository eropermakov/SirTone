import { sfx } from "@/shared/ipc/sfx";
import { cn } from "@/shared/lib/utils";

type Props = {
  checked: boolean;
  onChange: (on: boolean) => void;
  label: string;
  disabled?: boolean;
};

/** On/off switch: `accent-fill` when on (DESIGN.md §2, 10 %), transform-only motion (§10). */
export function Switch({ checked, onChange, label, disabled }: Props) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => {
        sfx(checked ? "toggle_off" : "toggle_on");
        onChange(!checked);
      }}
      className={cn(
        "relative h-6 w-11 shrink-0 rounded-full transition-colors disabled:opacity-40",
        checked ? "bg-accent-fill" : "bg-inactive",
      )}
    >
      <span
        aria-hidden
        className={cn(
          "absolute top-1 left-1 size-4 rounded-full transition-transform motion-reduce:transition-none",
          checked ? "translate-x-5 bg-accent-on" : "bg-text-primary",
        )}
      />
    </button>
  );
}
