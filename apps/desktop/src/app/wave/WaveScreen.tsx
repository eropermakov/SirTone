import {
  Ban,
  CircleSlash,
  Disc3,
  Heart,
  HelpCircle,
  ListPlus,
  MicVocal,
  Moon,
  MoreHorizontal,
  Pencil,
  Shuffle,
  Trash2,
  Waves,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { TrackCard } from "@/app/library/TrackCard";
import type { GoTo } from "@/app/library/TrackList";
import { useAddToPlaylist } from "@/app/playlists/useAddToPlaylist";
import { useTrackTags } from "@/app/playlists/useTrackTags";
import {
  like,
  never,
  notNow,
  primaryArtist,
  type TrackFeedback,
  trackFeedback,
} from "@/shared/ipc/feedback";
import { allTrackIds, playTracks, trackDetails, waveStart, waveWhy } from "@/shared/ipc/library";
import { type PlaybackSnapshot, setShuffle, togglePlay } from "@/shared/ipc/playback";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/Button";
import { Cover } from "@/shared/ui/Cover";
import { Menu, type MenuItem } from "@/shared/ui/Menu";
import { notify } from "@/shared/ui/notify";
import { PauseIcon, PlayIcon } from "@/shared/ui/TransportIcons";
import { ForAllButton, type Gathering } from "./ForAllButton";
import { NightVibe } from "./NightVibe";
import { NoveltySlider } from "./NoveltySlider";
import { SeasonWaveButton } from "./SeasonWaveButton";
import { VizBackground } from "./VizBackground";
import { VizToolbar } from "./VizToolbar";
import { pulseWaves } from "./WavesBackground";

const NONE: TrackFeedback = { liked: false, neverTrack: false, neverArtist: false };

/**
 * «Моя волна» as the player (T2.5, FR-WAVE-001; `06` §3 layout): background gradient stub, big
 * cover, title and artist, ♡ / «не сейчас» / «⋯» (T2.19, FR-LEARN-003), a big Play. The
 * timeline, transport and volume stay in the PlayerBar below (one set of controls, `06` §12).
 * «Слушать волну» runs the engine (Stage 3): it picks by taste, statistics and the signals and
 * adds more as the queue runs out. Chips, «Состав» and the slider UI come next.
 */
export function WaveScreen({ snap, onGo }: { snap: PlaybackSnapshot; onGo: GoTo }) {
  const { t } = useTranslation("shell");
  const id = snap.trackId ?? null;
  const [fb, setFb] = useState<TrackFeedback>(NONE);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [night, setNight] = useState(false);
  const leaveNight = useCallback(() => setNight(false), []);
  const [card, setCard] = useState<{ id: number; confirmDelete: boolean } | null>(null);
  const closeCard = useCallback(() => setCard(null), []);
  const toPlaylist = useAddToPlaylist();
  const tagsMenu = useTrackTags(id);
  const playing = snap.state === "playing";

  useEffect(() => {
    setFb(NONE);
    if (id == null) return;
    trackFeedback(id)
      .then(setFb)
      .catch(() => {});
  }, [id]);

  const startWave = (profile?: "seasonal" | "for_all", gathering?: Gathering | null) =>
    waveStart(profile, gathering ? { gathering } : undefined)
      .then((r) => {
        if (r.queued === 0) notify(t("wave.empty"));
        // «Для всех» is honest about its data (FR-ALL-005).
        else if (r.few) notify(t("wave.forAll.few"));
        else if (r.noPopularity) notify(t("wave.forAll.noPopularity"));
        // «Настроить волну» found too few tracks (FR-WAVE-006).
        else if (r.relaxed.length > 0) notify(t("wave.relaxed"));
      })
      .catch(() => notify(t("wave.failed")));
  const forAll = (g: Gathering | null) => void startWave("for_all", g);
  const shuffleAll = () =>
    allTrackIds().then((ids) => {
      if (ids.length === 0) return;
      setShuffle(true);
      playTracks(ids, Math.floor(Math.random() * ids.length), "collection");
    });

  if (id == null || snap.state === "empty") {
    return (
      <section className="flex h-full flex-col items-center justify-center gap-4 text-center">
        <h1 className="t-page-title text-primary-text">{t("wave.title")}</h1>
        <p className="t-body max-w-md text-text-secondary">{t("wave.idle")}</p>
        <div className="flex flex-wrap justify-center gap-3">
          <Button variant="primary" onClick={() => void startWave()}>
            <Waves size={16} strokeWidth={1.5} />
            {t("wave.start")}
          </Button>
          <SeasonWaveButton onStart={() => void startWave("seasonal")} />
          <ForAllButton onStart={forAll} />
          <Button variant="secondary" onClick={() => void shuffleAll()}>
            <Shuffle size={16} strokeWidth={1.5} />
            {t("wave.shuffleAll")}
          </Button>
        </div>
      </section>
    );
  }

  // Optimistic (DESIGN.md §1): the heart changes now, a failure puts it back.
  const toggleLike = () => {
    const on = !fb.liked;
    setFb((f) => ({ ...f, liked: on }));
    if (on) pulseWaves("like");
    like(id, on)
      .then(() => notify(t(on ? "wave.liked" : "wave.unliked")))
      .catch(() => {
        setFb((f) => ({ ...f, liked: !on }));
        notify(t("wave.failed"));
      });
  };
  const skipNotNow = () =>
    notNow(id)
      .then(() => notify(t("wave.notNowDone")))
      .catch(() => notify(t("wave.failed")));
  const setNever = (scope: "track" | "artist", on: boolean) =>
    (scope === "track" ? Promise.resolve(id) : primaryArtist(id))
      .then((target) => never(scope, target, on))
      .then(() => {
        setFb((f) => (scope === "track" ? { ...f, neverTrack: on } : { ...f, neverArtist: on }));
        notify(t(on ? "wave.neverOn" : "wave.neverOff"));
      })
      .catch(() => notify(t("wave.failed")));

  const why = () =>
    waveWhy(id)
      .then((rs) => {
        const days = t("wave.daypart", { returnObjects: true }) as unknown as string[];
        const genres = t("wave.genres", { returnObjects: true }) as unknown as string[];
        const seasons = t("wave.seasonName", { returnObjects: true }) as unknown as string[];
        const text = rs
          .map((r) =>
            t(`wave.why.${r.kind}`, {
              daypart: days[r.arg ?? 0] ?? "",
              genre: genres[r.arg ?? 0] ?? "",
              season: seasons[r.arg ?? 0] ?? "",
            }),
          )
          .join(" · ");
        notify(text || t("wave.why.none"));
      })
      .catch(() => notify(t("wave.failed")));
  const items: MenuItem[] = [
    ...(snap.inWave
      ? [{ id: "why", label: t("wave.whyMenu"), icon: HelpCircle, onSelect: () => void why() }]
      : []),
    {
      id: "playlist",
      label: t("trackMenu.addToPlaylist"),
      icon: ListPlus,
      submenu: toPlaylist.submenu([id]),
    },
    {
      id: "goto",
      label: t("trackMenu.goTo"),
      icon: Disc3,
      submenu: [
        {
          id: "album",
          label: t("trackMenu.toAlbum"),
          icon: Disc3,
          onSelect: () =>
            trackDetails(id)
              .then((d) => {
                if (d.albumId != null) onGo("albums", d.albumId, d.album ?? "");
              })
              .catch(() => {}),
        },
        {
          id: "artist",
          label: t("trackMenu.toArtist"),
          icon: MicVocal,
          onSelect: () =>
            primaryArtist(id).then((a) => onGo("artists", a, snap.artists?.split(", ")[0] ?? "")),
        },
      ],
    },
    {
      id: "details",
      label: t("trackMenu.details"),
      icon: Pencil,
      onSelect: () => setCard({ id, confirmDelete: false }),
    },
    tagsMenu,
    {
      id: "never",
      label: t("wave.never"),
      icon: Ban,
      submenu: [
        {
          id: "never-track",
          label: fb.neverTrack ? t("wave.neverTrackOff") : t("wave.neverTrack"),
          onSelect: () => void setNever("track", !fb.neverTrack),
        },
        {
          id: "never-artist",
          label: fb.neverArtist ? t("wave.neverArtistOff") : t("wave.neverArtist"),
          onSelect: () => void setNever("artist", !fb.neverArtist),
        },
      ],
    },
    { separator: true },
    {
      id: "remove",
      label: t("trackMenu.remove"),
      icon: Trash2,
      danger: true,
      onSelect: () => setCard({ id, confirmDelete: true }),
    },
  ];

  const PlayPause = playing ? PauseIcon : PlayIcon;
  return (
    <section
      data-theme="dark"
      data-viz-scope
      className="relative flex h-full min-h-0 flex-col items-center justify-center gap-8 overflow-hidden rounded-(--radius-xl) text-text-primary"
    >
      {/* The visualisation is the screen's background (T4.2, DESIGN.md §9: alabaster text). */}
      {/* One visualiser at a time: the night vibe takes the feed while it is open. */}
      {night ? (
        <NightVibe
          trackId={id}
          cover={snap.cover}
          title={snap.title}
          artists={snap.artists}
          onExit={leaveNight}
        />
      ) : (
        <VizBackground trackId={id} cover={snap.cover} />
      )}
      {!night && <VizToolbar />}
      <Cover
        hash={snap.cover}
        art={snap.title ? snap.art : undefined}
        px={280}
        rounded="md"
        className="relative rounded-(--radius-lg) shadow-(--shadow-artwork)"
      />
      <div className="relative flex max-w-xl flex-col items-center gap-1 text-center">
        <h1 className="t-hero line-clamp-2 text-text-primary">{snap.title}</h1>
        <button
          type="button"
          onClick={() =>
            primaryArtist(id)
              .then((a) => onGo("artists", a, snap.artists?.split(", ")[0] ?? ""))
              .catch(() => {})
          }
          className="t-body truncate text-text-secondary hover:text-text-primary"
        >
          {snap.artists}
        </button>
      </div>
      {/* Which wave plays; the other one starts from here (FR-SEAS-003). */}
      <div className="relative flex flex-wrap justify-center gap-3">
        {snap.waveProfile !== "my_wave" && (
          <Button variant="secondary" onClick={() => void startWave()} className="relative">
            <Waves size={16} strokeWidth={1.5} />
            {t("wave.start")}
          </Button>
        )}
        {snap.waveProfile !== "seasonal" && (
          <SeasonWaveButton onStart={() => void startWave("seasonal")} />
        )}
        <ForAllButton onStart={forAll} />
      </div>
      <div className="relative flex items-center gap-4">
        <button
          type="button"
          aria-label={t("wave.like")}
          aria-pressed={fb.liked}
          onClick={toggleLike}
          className={cn(
            "flex size-12 items-center justify-center rounded-full hover:bg-bg-hover",
            fb.liked ? "text-accent-text" : "text-icon",
          )}
        >
          <Heart size={24} strokeWidth={1.5} fill={fb.liked ? "currentColor" : "none"} />
        </button>
        <button
          type="button"
          onClick={togglePlay}
          aria-label={playing ? t("player.pause") : t("player.play")}
          className="flex size-16 items-center justify-center rounded-full bg-accent-fill text-accent-on transition-transform active:scale-95"
        >
          <PlayPause size={28} />
        </button>
        <button
          type="button"
          aria-label={t("wave.notNow")}
          title={t("wave.notNow")}
          onClick={() => void skipNotNow()}
          className="flex size-12 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
        >
          <CircleSlash size={24} strokeWidth={1.5} />
        </button>
        <button
          type="button"
          aria-label={t("wave.night")}
          title={t("wave.night")}
          onClick={() => setNight(true)}
          className="flex size-12 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
        >
          <Moon size={24} strokeWidth={1.5} />
        </button>
        <button
          type="button"
          aria-label={t("trackMenu.more")}
          onClick={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            setMenu({ x: r.left, y: r.bottom + 4 });
          }}
          className="flex size-12 items-center justify-center rounded-full text-icon hover:bg-bg-hover"
        >
          <MoreHorizontal size={24} strokeWidth={1.5} />
        </button>
      </div>
      <NoveltySlider />
      {menu && (
        <Menu items={items} at={menu} label={t("trackMenu.label")} onClose={() => setMenu(null)} />
      )}
      {toPlaylist.dialog}
      {card && (
        <TrackCard
          key={card.id}
          trackId={card.id}
          confirmDelete={card.confirmDelete}
          onClose={closeCard}
        />
      )}
    </section>
  );
}
