import { isTauri } from "@tauri-apps/api/core";
import { ChevronDown } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type GenreItem, type WaveQueryIpc } from "@/shared/ipc/bindings";
import { waveRetune } from "@/shared/ipc/library";
import { getSettings, patchSettings } from "@/shared/ipc/settings";
import { cn } from "@/shared/lib/utils";
import { Chip } from "@/shared/ui/Chip";
import { Slider } from "@/shared/ui/Slider";

const SPACINGS = [3, 5, 8, 12] as const;

/**
 * The expert level of «Настроить волну» (T5.16, FR-LEARN-012, FR-NOV-003, `14` §9): opened on
 * demand — the taste's weight, «неслушанность» and «непохожесть» as two sliders, genres never in
 * this wave, how often one artist may come. «Никогда» for artists and tracks and «Сбросить
 * обучение» stay in Settings (one place for them).
 */
export function WaveExpert({
  q,
  setQ,
}: {
  q: WaveQueryIpc;
  setQ: (f: (c: WaveQueryIpc) => WaveQueryIpc) => void;
}) {
  const { t } = useTranslation("shell");
  const [open, setOpen] = useState(false);
  const [genres, setGenres] = useState<GenreItem[]>([]);
  const [distance, setDistance] = useState<number | null>(null);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    if (!open) return;
    if (isTauri())
      commands
        .collectionGenres()
        .then((r) => r.status === "ok" && setGenres(r.data.slice(0, 40)))
        .catch(() => {});
    getSettings().then((s) => {
      const w = s.wave as { novelty_distance?: number | null } | undefined;
      setDistance(w?.novelty_distance ?? null);
    });
  }, [open]);
  const saveDistance = (v: number | null) => {
    setDistance(v);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      patchSettings({ wave: { novelty_distance: v } })
        .then(() => waveRetune())
        .catch(() => {});
    }, 250);
  };
  const excluded = q.excludeGenres ?? [];
  const taste = q.taste ?? 1;
  return (
    <section className="flex flex-col gap-3">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex items-center gap-2 self-start text-text-primary"
      >
        <span className="t-card-title">{t("wave.expert.title")}</span>
        <ChevronDown
          size={16}
          strokeWidth={1.5}
          className={cn("text-icon transition-transform", open && "rotate-180")}
        />
      </button>
      {open && (
        <div className="flex flex-col gap-4 rounded-(--radius-lg) bg-bg-layer p-4">
          <div className="flex items-center gap-3">
            <span className="t-body w-40 shrink-0 text-text-primary">{t("wave.expert.taste")}</span>
            <Slider
              value={taste / 2}
              onCommit={(v) => setQ((c) => ({ ...c, taste: Math.round(v * 20) / 10 }))}
              label={t("wave.expert.taste")}
              valueText={t("wave.setup.times", { value: taste.toFixed(1) })}
              className="flex-1"
            />
            <span className="t-caption w-12 shrink-0 text-right text-text-secondary">
              {t("wave.setup.times", { value: taste.toFixed(1) })}
            </span>
          </div>
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-3">
              <span className="t-body w-40 shrink-0 text-text-primary">
                {t("wave.expert.distance")}
              </span>
              <Slider
                value={distance == null ? 0.5 : distance / 100}
                onCommit={(v) => saveDistance(Math.round(v * 100))}
                label={t("wave.expert.distance")}
                valueText={distance == null ? t("wave.expert.followMain") : String(distance)}
                className="flex-1"
              />
              <Chip active={distance == null} onClick={() => saveDistance(null)}>
                {t("wave.expert.followMain")}
              </Chip>
            </div>
            <span className="t-caption text-text-secondary">{t("wave.expert.distanceHint")}</span>
          </div>
          <div className="flex flex-col gap-2">
            <span className="t-body text-text-primary">{t("wave.expert.spacing")}</span>
            <div className="flex flex-wrap gap-2">
              <Chip
                active={q.artistSpacing == null}
                onClick={() => setQ((c) => ({ ...c, artistSpacing: null }))}
              >
                {t("wave.expert.spacingProfile")}
              </Chip>
              {SPACINGS.map((n) => (
                <Chip
                  key={n}
                  active={q.artistSpacing === n}
                  onClick={() => setQ((c) => ({ ...c, artistSpacing: n }))}
                >
                  {t("wave.expert.spacingN", { count: n })}
                </Chip>
              ))}
            </div>
          </div>
          {genres.length > 0 && (
            <div className="flex flex-col gap-2">
              <span className="t-body text-text-primary">{t("wave.expert.genres")}</span>
              <div className="flex flex-wrap gap-2">
                {genres.map((g) => {
                  const key = g.name.trim().toLowerCase();
                  const off = excluded.includes(key);
                  return (
                    <Chip
                      key={g.key}
                      active={off}
                      onClick={() =>
                        setQ((c) => {
                          const list = c.excludeGenres ?? [];
                          return {
                            ...c,
                            excludeGenres: off ? list.filter((x) => x !== key) : [...list, key],
                          };
                        })
                      }
                    >
                      {g.name}
                    </Chip>
                  );
                })}
              </div>
            </div>
          )}
          <span className="t-caption text-text-secondary">{t("wave.expert.elsewhere")}</span>
        </div>
      )}
    </section>
  );
}
