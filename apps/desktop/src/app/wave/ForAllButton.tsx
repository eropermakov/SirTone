import { Coffee, Music, PartyPopper, Users } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/shared/ui/Button";
import { Menu } from "@/shared/ui/Menu";

export type Gathering = "background" | "company" | "party";

/**
 * «Для всех» (T3.32, FR-ALL-003/006, `14` §16): the wave for guests — tracks most people like,
 * nothing sad, gloomy or aggressive, the owner's taste untouched. The atmospheres «Фон»,
 * «Компания», «Вечеринка» narrow it (`wave_start` maps them onto the wave filters).
 */
export function ForAllButton({ onStart }: { onStart: (g: Gathering | null) => void }) {
  const { t } = useTranslation("shell");
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  return (
    <>
      <Button
        variant="secondary"
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setMenu({ x: r.left, y: r.bottom + 4 });
        }}
      >
        <Users size={16} strokeWidth={1.5} />
        {t("wave.forAll.button")}
      </Button>
      {menu && (
        <Menu
          at={menu}
          label={t("wave.forAll.menu")}
          onClose={() => setMenu(null)}
          items={[
            {
              id: "any",
              label: t("wave.forAll.any"),
              icon: Users,
              onSelect: () => onStart(null),
            },
            {
              id: "background",
              label: t("wave.forAll.background"),
              icon: Coffee,
              onSelect: () => onStart("background"),
            },
            {
              id: "company",
              label: t("wave.forAll.company"),
              icon: Music,
              onSelect: () => onStart("company"),
            },
            {
              id: "party",
              label: t("wave.forAll.party"),
              icon: PartyPopper,
              onSelect: () => onStart("party"),
            },
          ]}
        />
      )}
    </>
  );
}
