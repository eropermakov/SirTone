import { RefreshCw, Sparkles, Trash2, Undo2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  playTracks,
  removeTrack,
  type TrackItem,
  type UnknownItem,
  unknownAccept,
  unknownAssignArtist,
  unknownList,
  unknownNormal,
  unknownRelease,
  unknownRetry,
} from "@/shared/ipc/library";
import { Button } from "@/shared/ui/Button";
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
  "quarantined",
  "none",
] as const;

/**
 * «Неизвестные» (FR-LIB-004, FR-REC-008, FR-IMP-007/008; copy of Picard «Unmatched», WaveFlow
 * «Needs attention», `01b` F-08): the tracks without a confident identity, filtered by reason.
 * T3.11: hints in one click (found online, D-87), «Повторить онлайн», one artist for all shown,
 * «нормальный» for long files (AT-12), the quarantine for files that stopped playing — to the
 * Recycle Bin or back, never a silent delete. Files move through the journal; audio is untouched.
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
  const [selected, setSelected] = useState<UnknownItem | null>(null);
  const [massArtist, setMassArtist] = useState("");
  const [busy, setBusy] = useState(false);

  const load = () => {
    unknownList()
      .then(setItems)
      .catch(() => setItems([]));
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: version is the refetch trigger
  useEffect(load, [version]);

  const counts = useMemo(() => {
    const m = new Map<string, number>();
    for (const i of items) m.set(i.reason ?? "none", (m.get(i.reason ?? "none") ?? 0) + 1);
    return m;
  }, [items]);
  const shown = reason ? items.filter((i) => (i.reason ?? "none") === reason) : items;
  const tracks = shown.map((i) => i.track);
  const ids = tracks.map((x) => x.id);
  // The quarantine is not assigned in bulk (those files do not play).
  const assignable = shown.filter((i) => i.reason !== "quarantined").map((i) => i.track.id);
  const longOnes = shown.filter((i) => i.reason === "possible_non_track").map((i) => i.track.id);

  const drop = (ids: number[]) => {
    setItems((list) => list.filter((i) => !ids.includes(i.track.id)));
    setSelected(null);
  };

  const retry = () =>
    unknownRetry()
      .then((ok) => notify(t(ok ? "unknownScreen.retrying" : "unknownScreen.offline")))
      .catch(() => notify(t("unknownScreen.failed")));

  const massAssign = () => {
    const artist = massArtist.trim();
    if (!artist || assignable.length === 0) return;
    setBusy(true);
    unknownAssignArtist(assignable, artist)
      .then((n) => {
        notify(t("unknownScreen.massDone", { count: n }));
        setMassArtist("");
        drop(assignable);
      })
      .catch(() => notify(t("unknownScreen.failed")))
      .finally(() => setBusy(false));
  };

  const normal = (list: number[]) =>
    unknownNormal(list)
      .then((n) => {
        notify(t("unknownScreen.normalDone", { count: n }));
        load();
      })
      .catch(() => notify(t("unknownScreen.failed")));

  return (
    <section className="flex h-full min-h-0 gap-6">
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex items-center justify-between gap-4">
          <h1 className="t-page-title text-primary-text">{t("empty.unknown.title")}</h1>
          <Button variant="secondary" onClick={() => void retry()}>
            <RefreshCw size={16} strokeWidth={1.5} />
            {t("unknownScreen.retry")}
          </Button>
        </div>
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
        {assignable.length > 0 && (
          <div className="flex flex-wrap items-center gap-2 pb-2">
            <input
              value={massArtist}
              onChange={(e) => setMassArtist(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") massAssign();
              }}
              aria-label={t("unknownScreen.massTitle")}
              placeholder={t("unknownScreen.massTitle")}
              className="t-body h-(--button-height) min-w-0 flex-1 rounded-(--radius-md) border border-border bg-bg-layer px-3 text-text-primary placeholder:text-text-tertiary"
            />
            <Button
              variant="secondary"
              disabled={busy || massArtist.trim() === ""}
              onClick={massAssign}
            >
              {t("unknownScreen.massApply", { count: assignable.length })}
            </Button>
            {longOnes.length > 0 && (
              <Button variant="secondary" onClick={() => void normal(longOnes)}>
                {t("unknownScreen.normalAll")}
              </Button>
            )}
          </div>
        )}
        <TrackList
          tracks={tracks}
          playingId={playingId}
          label={t("collection.trackList")}
          onPlay={(i) => playTracks(ids, i, "collection")}
          context="collection"
          onGo={onGo}
          onSelect={(i) => setSelected(shown[i] ?? null)}
        />
      </div>
      <AssignPanel
        key={selected?.track.id ?? 0}
        item={selected}
        onNormal={(id) => void normal([id])}
        onDone={(title, key) => {
          notify(t(key, { title }));
          if (selected) drop([selected.track.id]);
        }}
      />
    </section>
  );
}

/** Neutral Panel (DESIGN.md §9): hints, the assignment form, or the quarantine choice. */
function AssignPanel({
  item,
  onNormal,
  onDone,
}: {
  item: UnknownItem | null;
  onNormal: (id: number) => void;
  onDone: (title: string, key: string) => void;
}) {
  const { t } = useTranslation("shell");
  if (!item) {
    return (
      <aside className="flex w-80 shrink-0 items-center justify-center rounded-(--radius-xl) border border-border bg-bg-raised p-6">
        <p className="t-body text-center text-text-secondary">{t("unknownScreen.pick")}</p>
      </aside>
    );
  }
  const track: TrackItem = item.track;
  if (item.reason === "quarantined") {
    return (
      <aside
        aria-label={t("unknownScreen.reason.quarantined")}
        className="flex w-80 shrink-0 flex-col gap-3 self-start rounded-(--radius-xl) border border-border bg-bg-raised p-6"
      >
        <h2 className="t-card-title text-text-primary">{track.title}</h2>
        <p className="t-body text-text-secondary">{t("unknownScreen.quarantineHint")}</p>
        <Button
          variant="secondary"
          className="text-danger"
          onClick={() =>
            removeTrack(track.id, true)
              .then(() => onDone(track.title, "unknownScreen.trashed"))
              .catch(() => notify(t("unknownScreen.failed")))
          }
        >
          <Trash2 size={16} strokeWidth={1.5} />
          {t("unknownScreen.trash")}
        </Button>
        <Button
          variant="ghost"
          onClick={() =>
            unknownRelease(track.id)
              .then(() => onDone(track.title, "unknownScreen.released"))
              .catch(() => notify(t("unknownScreen.failed")))
          }
        >
          <Undo2 size={16} strokeWidth={1.5} />
          {t("unknownScreen.release")}
        </Button>
      </aside>
    );
  }
  return (
    <aside
      aria-label={t("unknownScreen.assignTitle")}
      className="flex w-80 shrink-0 flex-col gap-3 self-start rounded-(--radius-xl) border border-border bg-bg-raised p-6"
    >
      {item.reason === "possible_non_track" && (
        <>
          <p className="t-caption text-text-secondary">{t("unknownScreen.nonTrackHint")}</p>
          <Button variant="secondary" onClick={() => onNormal(track.id)}>
            {t("unknownScreen.normal")}
          </Button>
        </>
      )}
      {item.candidates.length > 0 && (
        <div className="flex flex-col gap-2">
          <span className="t-caption text-text-secondary">{t("unknownScreen.maybe")}</span>
          {item.candidates.map((c, i) => (
            <Button
              // biome-ignore lint/suspicious/noArrayIndexKey: the hint's place is its address
              key={i}
              variant="secondary"
              className="h-auto justify-start py-2 text-left"
              onClick={() =>
                unknownAccept(track.id, i)
                  .then(() => onDone(c.title, "unknownScreen.accepted"))
                  .catch(() => notify(t("unknownScreen.failed")))
              }
            >
              <Sparkles size={16} strokeWidth={1.5} />
              <span className="flex min-w-0 flex-col">
                <span className="t-body truncate">
                  {t("unknownScreen.accept", { title: c.title, artists: c.artists })}
                </span>
                {c.online && (
                  <span className="t-caption text-text-tertiary">{t("unknownScreen.online")}</span>
                )}
              </span>
            </Button>
          ))}
        </div>
      )}
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
        onDone={(title) => onDone(title, "unknownScreen.assigned")}
      />
    </aside>
  );
}
