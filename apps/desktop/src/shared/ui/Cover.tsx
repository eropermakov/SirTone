import {
  AudioWaveform,
  Disc3,
  Guitar,
  Leaf,
  type LucideIcon,
  MicVocal,
  Music2,
  Piano,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { cn } from "@/shared/lib/utils";

/** Sizes the `sirtone-cover` protocol serves (docs/03 §5.6). */
const SIZES = [64, 128, 256, 512, 1024] as const;

/** Smallest served size that covers `px` CSS pixels on this display. */
export function coverUrl(hash: string, px: number): string {
  const need = px * (window.devicePixelRatio || 1);
  const size = SIZES.find((s) => s >= need) ?? 1024;
  return `http://sirtone-cover.localhost/${hash}/${size}`;
}

/** Genre groups (`14` §4.1): поп/танц. · рок/метал · электроника · хип-хоп/R&B ·
 * классика/джаз/эмбиент · фолк/прочее. */
const GENRE_ICONS: LucideIcon[] = [Disc3, Guitar, AudioWaveform, MicVocal, Piano, Leaf];
/** How much of the accent the gradient carries by mood: none · весёлое · бодрое · спокойное ·
 * грустное (temporary until the owner's placeholder style, D-9 / D-89). */
const MOOD_ACCENT = [40, 55, 70, 30, 20];

/**
 * Album artwork (DESIGN.md §9): the cached WebP from `sirtone-cover://`; without one — a
 * generated placeholder (T3.20, FR-UI-006): the genre group's sign on a gradient of the accent
 * into `artwork-placeholder`, stronger for brighter moods; unknown genre and mood — the base
 * placeholder with the logo letter. `art` = `genre << 4 | mood` from the library.
 */
export function Cover({
  hash,
  px,
  className,
  rounded = "sm",
  art,
}: {
  hash: string | null | undefined;
  px: number;
  className?: string;
  rounded?: "sm" | "md" | "full";
  /** Placeholder hint; absent — the plain note placeholder. */
  art?: number | undefined;
}) {
  const { t } = useTranslation("common");
  const [failed, setFailed] = useState<string | null>(null);
  const radius =
    rounded === "full"
      ? "rounded-full"
      : rounded === "md"
        ? "rounded-(--radius-md)"
        : "rounded-(--radius-sm)";
  const box = cn("shrink-0 overflow-hidden", radius, className);
  if (!hash || failed === hash) {
    if (art === undefined) {
      return (
        <span
          className={cn(
            box,
            "flex items-center justify-center bg-artwork-placeholder text-text-tertiary",
          )}
          style={{ width: px, height: px }}
        >
          <Music2 size={Math.max(16, Math.round(px / 3))} strokeWidth={1.5} />
        </span>
      );
    }
    const genre = art >> 4;
    const mood = art & 15;
    const Icon = GENRE_ICONS[genre];
    const accent = MOOD_ACCENT[mood] ?? MOOD_ACCENT[0];
    const known = Icon !== undefined || mood > 0;
    return (
      <span
        role="img"
        aria-hidden
        data-art={art}
        className={cn(box, "flex items-center justify-center text-text-secondary")}
        style={{
          width: px,
          height: px,
          background: known
            ? `linear-gradient(135deg, color-mix(in oklch, var(--accent-fill) ${accent}%, var(--artwork-placeholder)), var(--artwork-placeholder))`
            : "var(--artwork-placeholder)",
        }}
      >
        {Icon ? (
          <Icon size={Math.max(16, Math.round(px / 3))} strokeWidth={1.5} />
        ) : (
          // The base placeholder: the logo letter (D-4 temporary logo «С»).
          <span className="t-card-title leading-none" style={{ fontSize: Math.round(px / 2.6) }}>
            {t("app.letter")}
          </span>
        )}
      </span>
    );
  }
  return (
    <img
      src={coverUrl(hash, px)}
      width={px}
      height={px}
      alt=""
      loading="lazy"
      decoding="async"
      onError={() => setFailed(hash)}
      className={cn(box, "bg-artwork-placeholder object-cover")}
    />
  );
}
