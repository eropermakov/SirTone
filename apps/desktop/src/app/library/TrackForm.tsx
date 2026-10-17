import { useState } from "react";
import { useTranslation } from "react-i18next";
import { assignTrack } from "@/shared/ipc/library";
import { Button } from "@/shared/ui/Button";

export type TrackFormValues = {
  title: string;
  artists: string;
  album: string;
  trackNo: string;
  year: string;
};

const WIDE = [
  ["title", "unknownScreen.title"],
  ["artists", "unknownScreen.artists"],
  ["album", "unknownScreen.album"],
] as const;
const NARROW = [
  ["trackNo", "unknownScreen.trackNo"],
  ["year", "unknownScreen.year"],
] as const;

/**
 * «Кто это и что это» (FR-LIB-007, FR-REC-008): title, artists (comma separated), album, number,
 * year. Saving moves the file to its canonical place through the journal; the audio file itself
 * is never modified. Shared by «Неизвестные» and the track card.
 */
export function TrackForm({
  trackId,
  initial,
  submitLabel,
  onDone,
}: {
  trackId: number;
  initial: TrackFormValues;
  submitLabel: string;
  onDone: (title: string) => void;
}) {
  const { t } = useTranslation("shell");
  const [v, setV] = useState(initial);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const set = (k: keyof TrackFormValues) => (value: string) => setV((x) => ({ ...x, [k]: value }));
  const num = (s: string) => (/^\d+$/.test(s.trim()) ? Number(s.trim()) : null);
  const ready = v.title.trim() !== "" && v.artists.trim() !== "";
  const submit = () => {
    setBusy(true);
    setFailed(false);
    assignTrack(trackId, {
      title: v.title,
      artists: v.artists,
      album: v.album.trim() || null,
      trackNo: num(v.trackNo),
      year: num(v.year),
    })
      .then(() => onDone(v.title))
      .catch(() => setFailed(true))
      .finally(() => setBusy(false));
  };
  const field = (
    label: string,
    k: keyof TrackFormValues,
    opts: { hint?: string; narrow?: boolean } = {},
  ) => (
    <label key={k} className={opts.narrow ? "flex w-20 flex-col gap-1" : "flex flex-col gap-1"}>
      <span className="t-caption text-text-secondary">{label}</span>
      <input
        value={v[k]}
        onChange={(e) => set(k)(e.target.value)}
        inputMode={opts.narrow ? "numeric" : undefined}
        className="t-body h-(--button-height) rounded-(--radius-md) border border-border bg-bg-layer px-3 text-text-primary"
      />
      {opts.hint && <span className="t-caption text-text-tertiary">{opts.hint}</span>}
    </label>
  );
  return (
    <div className="flex flex-col gap-3">
      {WIDE.map(([k, key]) =>
        field(t(key), k, k === "artists" ? { hint: t("unknownScreen.artistsHint") } : {}),
      )}
      <div className="flex gap-3">
        {NARROW.map(([k, key]) => field(t(key), k, { narrow: true }))}
      </div>
      {failed && <p className="t-caption text-danger">{t("unknownScreen.failed")}</p>}
      <Button variant="primary" disabled={!ready || busy} onClick={submit} className="self-start">
        {submitLabel}
      </Button>
    </div>
  );
}
