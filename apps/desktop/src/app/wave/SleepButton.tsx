import { Moon, Timer } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { sleepCancel, sleepStart, useSleepStatus } from "@/shared/ipc/sleep";
import { cn } from "@/shared/lib/utils";
import { Menu, type MenuItem } from "@/shared/ui/Menu";

/** Spotify / YouTube Music presets (`01c` W-08), minutes. */
const MINUTES = [15, 30, 45, 60, 90] as const;
/** «Через N треков». */
const TRACKS = 3;

/**
 * «Сон-таймер» on the wave screen (T4.21, FR-SLEEP-001/002): one press opens the choices —
 * minutes, «до конца трека», «через 3 трека», and «Засыпательная волна» (the sleep wave with
 * a 30-minute timer, FR-SLEEP-003); while it runs — «Отменить таймер». Temporary look until the
 * owner's design (D-48, `06` §12).
 */
export function SleepButton({ onSleepWave }: { onSleepWave: () => void }) {
  const { t } = useTranslation("shell");
  const { s } = useSleepStatus();
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  const on = s.phase !== "off" && s.phase !== "ended";
  const items: MenuItem[] = [
    ...MINUTES.map((m) => ({
      id: `m${m}`,
      label: t("sleep.minutes", { count: m }),
      onSelect: () => void sleepStart("minutes", m),
    })),
    { id: "track", label: t("sleep.endOfTrack"), onSelect: () => void sleepStart("track", 1) },
    {
      id: "tracks",
      label: t("sleep.tracks", { count: TRACKS }),
      onSelect: () => void sleepStart("tracks", TRACKS),
    },
    { separator: true },
    { id: "wave", label: t("sleep.wave"), icon: Moon, onSelect: onSleepWave },
    ...(on
      ? [
          { separator: true } as const,
          {
            id: "cancel",
            label: t("sleep.cancel"),
            danger: true,
            onSelect: () => void sleepCancel(),
          },
        ]
      : []),
  ];
  return (
    <>
      <button
        type="button"
        aria-label={t("sleep.button")}
        title={t("sleep.button")}
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
        <Timer size={24} strokeWidth={1.5} />
      </button>
      {at && <Menu items={items} at={at} label={t("sleep.button")} onClose={() => setAt(null)} />}
    </>
  );
}
