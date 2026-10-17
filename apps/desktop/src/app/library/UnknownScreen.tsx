import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { playTracks, type TrackItem, type UnknownItem, unknownList } from "@/shared/ipc/library";
import { Chip } from "@/shared/ui/Chip";
import { notify } from "@/shared/ui/notify";
import { TrackForm } from "./TrackForm";
import { type GoTo, TrackList } from "./TrackList";

const REASONS = [
  "no_metadata",
  "conflict",
  "low_confidence",
  "offline_pending",
  "possible_non_track",
  "needs_review",
  "none",
] as const;

/**
 * «Неизвестные» (FR-LIB-004, FR-REC-008; copy of Picard «Unmatched», WaveFlow «Needs attention»,
 * `01b` F-08): the tracks without a confident identity, filtered by reason; pick one and say who
 * it is — the file moves to the artist's folder (through the journal; the audio is untouched).
 * Candidates in one click and «повторить онлайн» come with recognition online (Stage 3).
 */
export function UnknownScreen({
  version,
  playingId,
  onGo,
}: {
  version: number;
  playingId: number | null;
  onGo?: GoTo | undefined;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const [items, setItems] = useState<UnknownItem[]>([]);
  const [reason, setReason] = useState<string | null>(null);
  const [selected, setSelected] = useState<TrackItem | null>(null);

  useEffect(() => {
    void version;
    unknownList()
      .then(setItems)
      .catch(() => setItems([]));
  }, [version]);

  const counts = useMemo(() => {
    const m = new Map<string, number>();
    for (const i of items) m.set(i.reason ?? "none", (m.get(i.reason ?? "none") ?? 0) + 1);
    return m;
  }, [items]);
  const shown = reason ? items.filter((i) => (i.reason ?? "none") === reason) : items;
  const tracks = shown.map((i) => i.track);
  const ids = tracks.map((x) => x.id);

  return (
    <section className="flex h-full min-h-0 gap-6">
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <h1 className="t-page-title text-primary-text">{t("empty.unknown.title")}</h1>
        <p className="t-caption text-text-secondary">
          {tc("count.tracks", { count: items.length })} · {t("unknownScreen.hint")}
        </p>
        <div className="flex flex-wrap gap-2 py-2">
          <Chip active={reason === null} onClick={() => setReason(null)}>
            {t("unknownScreen.all")} {items.length}
          </Chip>
          {REASONS.filter((r) => (counts.get(r) ?? 0) > 0).map((r) => (
            <Chip key={r} active={reason === r} onClick={() => setReason(r)}>
              {t(`unknownScreen.reason.${r}`)} {counts.get(r)}
            </Chip>
          ))}
        </div>
        <TrackList
          tracks={tracks}
          playingId={playingId}
          label={t("collection.trackList")}
          onPlay={(i) => playTracks(ids, i, "collection")}
          context="collection"
          onGo={onGo}
          onSelect={(i) => setSelected(tracks[i] ?? null)}
        />
      </div>
      <AssignPanel
        key={selected?.id ?? 0}
        track={selected}
        onDone={(title) => {
          notify(t("unknownScreen.assigned", { title }));
          setSelected(null);
          setItems((list) => list.filter((i) => i.track.id !== selected?.id));
        }}
      />
    </section>
  );
}

/** Neutral Panel (DESIGN.md §9) with the assignment form; one Primary button. */
function AssignPanel({
  track,
  onDone,
}: {
  track: TrackItem | null;
  onDone: (title: string) => void;
}) {
  const { t } = useTranslation("shell");
  if (!track) {
    return (
      <aside className="flex w-80 shrink-0 items-center justify-center rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <p className="t-body text-center text-text-secondary">{t("unknownScreen.pick")}</p>
      </aside>
    );
  }
  return (
    <aside
      aria-label={t("unknownScreen.assignTitle")}
      className="flex w-80 shrink-0 flex-col gap-3 self-start rounded-(--radius-xl) border border-border bg-bg-raised p-6"
    >
      <h2 className="t-card-title text-text-primary">{t("unknownScreen.assignTitle")}</h2>
      <TrackForm
        trackId={track.id}
        initial={{
          title: track.title,
          artists: track.artists,
          album: track.album ?? "",
          trackNo: "",
          year: "",
        }}
        submitLabel={t("unknownScreen.assign")}
        onDone={onDone}
      />
    </aside>
  );
}
