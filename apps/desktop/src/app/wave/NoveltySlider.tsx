import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { waveRetune } from "@/shared/ipc/library";
import { getSettings, patchSettings } from "@/shared/ipc/settings";
import { Slider } from "@/shared/ui/Slider";

/**
 * «Знакомое ↔ Неожиданное» (T3.31, FR-NOV-002, docs/14 §9): one scale 0–100 sets the shares of
 * familiar / forgotten / new tracks, the distance from the taste and the exploration; the
 * upcoming wave tracks follow at once. Saved in `wave.novelty`.
 */
export function NoveltySlider() {
  const { t } = useTranslation("shell");
  const [value, setValue] = useState(0.5);
  const timer = useRef<number | undefined>(undefined);
  useEffect(() => {
    getSettings().then((s) => {
      const w = s.wave as { novelty?: number } | undefined;
      setValue((w?.novelty ?? 50) / 100);
    });
  }, []);
  const commit = (v: number) => {
    setValue(v);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      patchSettings({ wave: { novelty: Math.round(v * 100) } })
        .then(() => waveRetune())
        .catch(() => {});
    }, 250);
  };
  return (
    <div className="relative flex w-full max-w-md items-center gap-3">
      <span className="t-caption shrink-0 text-text-secondary">{t("wave.familiar")}</span>
      <Slider
        value={value}
        onCommit={commit}
        label={t("wave.novelty")}
        valueText={String(Math.round(value * 100))}
        step={0.05}
        className="flex-1"
      />
      <span className="t-caption shrink-0 text-text-secondary">{t("wave.unexpected")}</span>
    </div>
  );
}
