import { Target } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { focusStart, focusStop, useFocusStatus } from "@/shared/ipc/focus";
import { cn } from "@/shared/lib/utils";
import { Menu, type MenuItem } from "@/shared/ui/Menu";

/** Presets (FR-FOCUS-001, Pomofocus / the Pomodoro method): work / break minutes. */
const PRESETS = [
  [25, 5],
  [50, 10],
  [90, 20],
] as const;

/**
 * «Фокус» on the wave screen (T4.24): presets 25/5, 50/10, 90/20 and «Свой» (the settings); while
 * it runs — «Остановить фокус». Temporary look until the owner's design (D-104, `06` §12).
 */
export function FocusButton() {
  const { t } = useTranslation("shell");
  const { s } = useFocusStatus();
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  const on = s.phase !== "off";
  const items: MenuItem[] = on
    ? [{ id: "stop", label: t("focus.stop"), danger: true, onSelect: () => void focusStop() }]
    : [
        ...PRESETS.map(([w, b]) => ({
          id: `p${w}`,
          label: t("focus.preset", { work: w, brk: b }),
          onSelect: () => void focusStart(w, b),
        })),
        { id: "own", label: t("focus.own"), onSelect: () => void focusStart(0, 0) },
      ];
  return (
    <>
      <button
        type="button"
        aria-label={t("focus.button")}
        title={t("focus.button")}
        aria-pressed={on}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setAt({ x: r.left, y: r.bottom + 4 });
        }}
        className={cn(
          "flex size-12 items-center justify-center rounded-full hover:bg-bg-hover",
          on ? "text-accent-text" : "text-icon",
        )}
      >
        <Target size={24} strokeWidth={1.5} />
      </button>
      {at && <Menu items={items} at={at} label={t("focus.button")} onClose={() => setAt(null)} />}
    </>
  );
}
