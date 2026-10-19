import { Check, Heart, Lock, LockOpen, SkipForward, Sparkles, X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { patchSettings } from "@/shared/ipc/settings";
import { videoClips, videoRemove, videoSetMood } from "@/shared/ipc/viz";
import { cn } from "@/shared/lib/utils";
import { Menu, type MenuItem } from "@/shared/ui/Menu";
import { milkdropControl, useMilkdropControl } from "./milkdropControl";
import { useVizEnv, type VizMode } from "./useVizEnv";
import { useVideoControl, videoControl } from "./videoControl";

const MODES: VizMode[] = ["waves", "milkdrop", "video"];
const MOODS = ["any", "happy", "energetic", "calm", "sad"] as const;
const btn =
  "flex size-10 items-center justify-center rounded-full text-icon hover:bg-bg-hover aria-pressed:text-accent-text";

/**
 * The visualiser's own controls on «Моя волна» (FR-VIZ-001/004): the mode — «Волны», «MilkDrop»,
 * «Видео» — switched on the fly; in MilkDrop also the lock, ♥ / ✕ for the preset on screen and
 * «следующий» (Kodi Lock/Rate, Poweramp «замок», `01a` §B.2).
 */
export function VizToolbar() {
  const { t } = useTranslation("shell");
  const { prefs } = useVizEnv();
  const md = useMilkdropControl();
  const vc = useVideoControl();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const mode = prefs?.mode ?? "waves";
  const items: MenuItem[] = MODES.map((m) => ({
    id: m,
    label: t(`viz.mode.${m}`),
    icon: m === mode ? Check : undefined,
    onSelect: () => void patchSettings({ viz: { mode: m } }).catch(() => {}),
  }));
  if (mode === "video") {
    const clip = vc.clips.find((c) => c.id === vc.current);
    const refresh = () => videoClips().then((clips) => videoControl.set({ clips }));
    items.push({ separator: true });
    if (clip) {
      items.push(
        {
          id: "video-mood",
          label: t("viz.videoMood"),
          submenu: MOODS.map((m) => ({
            id: `mood-${m}`,
            label: t(`viz.moods.${m}`),
            icon: clip.mood === m ? Check : undefined,
            onSelect: () => void videoSetMood(clip.id, m).then(refresh),
          })),
        },
        {
          id: "video-remove",
          label: t("viz.videoRemove"),
          danger: true,
          onSelect: () => void videoRemove(clip.id).then(refresh),
        },
      );
    }
    items.push({ id: "video-hint", label: t("viz.videoHint"), disabled: true });
  }
  return (
    <div className="absolute top-4 right-4 flex items-center gap-1">
      {mode === "milkdrop" && md.preset && (
        <>
          <button
            type="button"
            aria-label={t("viz.lock")}
            title={t("viz.lock")}
            aria-pressed={md.locked}
            onClick={() => milkdropControl.toggleLock()}
            className={btn}
          >
            {md.locked ? (
              <Lock size={20} strokeWidth={1.5} />
            ) : (
              <LockOpen size={20} strokeWidth={1.5} />
            )}
          </button>
          <button
            type="button"
            aria-label={t("viz.like")}
            title={t("viz.like")}
            aria-pressed={md.rating > 0}
            onClick={() => milkdropControl.rate(md.rating > 0 ? 0 : 1)}
            className={btn}
          >
            <Heart size={20} strokeWidth={1.5} fill={md.rating > 0 ? "currentColor" : "none"} />
          </button>
          <button
            type="button"
            aria-label={t("viz.dislike")}
            title={t("viz.dislike")}
            onClick={() => milkdropControl.rate(-1)}
            className={btn}
          >
            <X size={20} strokeWidth={1.5} />
          </button>
          <button
            type="button"
            aria-label={t("viz.next")}
            title={t("viz.next")}
            onClick={() => milkdropControl.skip()}
            className={btn}
          >
            <SkipForward size={20} strokeWidth={1.5} />
          </button>
        </>
      )}
      <button
        type="button"
        aria-label={t("viz.modeMenu")}
        title={t("viz.modeMenu")}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setMenu({ x: r.right - 200, y: r.bottom + 4 });
        }}
        className={cn(btn)}
      >
        <Sparkles size={20} strokeWidth={1.5} />
      </button>
      {menu && (
        <Menu items={items} at={menu} label={t("viz.modeMenu")} onClose={() => setMenu(null)} />
      )}
    </div>
  );
}
