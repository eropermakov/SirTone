import { Flower2, Leaf, Snowflake, Sun } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { waveSeasonCard } from "@/shared/ipc/library";
import { Button } from "@/shared/ui/Button";

/** The season a card key belongs to (the dominant one of the blend), for the icon. */
const ICON: Record<string, typeof Leaf> = {
  winter: Snowflake,
  earlyWinter: Snowflake,
  lateWinter: Snowflake,
  spring: Flower2,
  earlySpring: Flower2,
  lateSpring: Flower2,
  summer: Sun,
  earlySummer: Sun,
  lateSummer: Sun,
  autumn: Leaf,
  earlyAutumn: Leaf,
  preWinter: Leaf,
};

/**
 * «Сезонная волна» (T3.30, FR-SEAS-003/004): a separate start next to «Моя волна», named by the
 * season or, in a transition window, by the mix («Предзимье», `14` §10.3; the name comes from
 * the engine's calendar).
 */
export function SeasonWaveButton({ onStart }: { onStart: () => void }) {
  const { t } = useTranslation("shell");
  const [key, setKey] = useState<string | null>(null);
  useEffect(() => {
    waveSeasonCard()
      .then(setKey)
      .catch(() => {});
  }, []);
  if (!key) return null;
  const Icon = ICON[key] ?? Leaf;
  return (
    <Button variant="secondary" onClick={onStart} className="relative">
      <Icon size={16} strokeWidth={1.5} />
      {t(`wave.season.${key}`)}
    </Button>
  );
}
