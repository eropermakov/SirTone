import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { type ArtistSuggestion, artistSuggestions, dismissSuggestion } from "@/shared/ipc/artists";
import { Button } from "@/shared/ui/Button";
import { runMerge } from "./ArtistMerge";

/**
 * «Похоже, это один исполнитель» (T3.12, FR-ART-003): names that differ only by script or
 * «The» («Bi-2» / «Би-2»). «Объединить» keeps the one with more tracks; «Не предлагать»
 * remembers the pair.
 */
export function MergeSuggestion({ version }: { version: number }) {
  const { t } = useTranslation("shell");
  const [list, setList] = useState<ArtistSuggestion[]>([]);
  useEffect(() => {
    void version;
    artistSuggestions()
      .then(setList)
      .catch(() => setList([]));
  }, [version]);
  const s = list[0];
  if (!s) return null;
  const next = () => setList((l) => l.slice(1));
  return (
    <div
      role="status"
      className="flex flex-wrap items-center gap-3 rounded-(--radius-md) border border-border bg-bg-raised px-4 py-3"
    >
      <span className="t-body min-w-0 flex-1 text-text-primary">
        {t("artistMerge.suggest", { a: s.keep.name, b: s.drop.name })}
      </span>
      <Button
        variant="secondary"
        onClick={() => {
          next();
          void runMerge(t, s.keep, s.drop);
        }}
      >
        {t("artistMerge.merge")}
      </Button>
      <Button
        variant="ghost"
        onClick={() => {
          next();
          void dismissSuggestion(s.keep.id, s.drop.id);
        }}
      >
        {t("artistMerge.dismiss")}
      </Button>
    </div>
  );
}
