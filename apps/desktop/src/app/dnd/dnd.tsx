import {
  DndContext,
  type DragEndEvent,
  type DragOverEvent,
  DragOverlay,
  type DragStartEvent,
  PointerSensor,
  pointerWithin,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import { Music2 } from "lucide-react";
import { type ReactNode, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useAddToPlaylist } from "@/app/playlists/useAddToPlaylist";
import { usePlaylists } from "@/app/playlists/usePlaylists";
import type { Screen } from "@/app/shell/Sidebar";
import { assignTrack, trackDetails } from "@/shared/ipc/library";
import { movePlaylistItem } from "@/shared/ipc/playlists";
import { Button } from "@/shared/ui/Button";
import { Dialog } from "@/shared/ui/Dialog";
import { notify } from "@/shared/ui/notify";

/**
 * Drag and drop inside the library (T2.8, FR-LIB-003; D-07 pointer events via `@dnd-kit/core`;
 * `01b` F-35): a track row is dragged after 6 px (click and double click stay), a ghost follows
 * the pointer; targets — a sidebar playlist (add, F-13 question on repeats), an artist card
 * (assign, confirmed: the file moves through the journal), a row of the same playlist
 * (reorder). Sidebar items spring open after 0.8 s (Finder). Esc cancels (dnd-kit default).
 */
export type DragTrack = {
  id: number;
  title: string;
  /** Dragged from a playlist page: reorder within it. */
  from?: { playlist: number; position: number };
};

/** Drop target ids. */
export const dropId = {
  playlist: (id: number) => `playlist:${id}`,
  artist: (id: number) => `artist:${id}`,
  row: (playlist: number, position: number) => `plrow:${playlist}:${position}`,
  nav: (screen: Screen) => `nav:${screen}`,
};

const SPRING_MS = 800;

export function useTrackDrag(key: string, data: DragTrack) {
  const { setNodeRef, listeners, isDragging } = useDraggable({ id: `track:${key}`, data });
  return { ref: setNodeRef, listeners, isDragging };
}

export function useDropTarget(id: string, data?: Record<string, unknown>) {
  const { setNodeRef, isOver, active } = useDroppable({ id, data: data ?? {} });
  return { ref: setNodeRef, isOver: isOver && active != null };
}

type Assign = { track: DragTrack; artist: { id: number; name: string } };

export function DndRoot({
  children,
  onNavigate,
}: {
  children: ReactNode;
  onNavigate: (s: Screen) => void;
}) {
  const { t } = useTranslation("shell");
  const { t: tc } = useTranslation("common");
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }));
  const [dragging, setDragging] = useState<DragTrack | null>(null);
  const [assign, setAssign] = useState<Assign | null>(null);
  const toPlaylist = useAddToPlaylist();
  const lists = usePlaylists();
  const spring = useRef<{ id: string; timer: number } | null>(null);
  // Kept from the start: spring-loading changes the screen and unmounts the source row, and
  // dnd-kit's `active.data` goes with it.
  const carried = useRef<DragTrack | null>(null);

  const clearSpring = () => {
    if (spring.current) window.clearTimeout(spring.current.timer);
    spring.current = null;
  };

  const onStart = (e: DragStartEvent) => {
    const d = (e.active.data.current as DragTrack | undefined) ?? null;
    carried.current = d ? { ...d } : null;
    setDragging(carried.current);
  };

  const onOver = (e: DragOverEvent) => {
    const id = e.over ? String(e.over.id) : "";
    if (spring.current?.id === id) return;
    clearSpring();
    if (id.startsWith("nav:")) {
      const screen = id.slice(4) as Screen;
      spring.current = { id, timer: window.setTimeout(() => onNavigate(screen), SPRING_MS) };
    }
  };

  const onEnd = (e: DragEndEvent) => {
    clearSpring();
    setDragging(null);
    const track = carried.current;
    carried.current = null;
    const over = e.over ? String(e.over.id) : "";
    if (!track || !over) return;
    const [kind, a, b] = over.split(":");
    if (kind === "playlist") {
      const id = Number(a);
      const name = lists.find((p) => p.id === id)?.name ?? "";
      toPlaylist.add(id, name, [track.id]);
    } else if (kind === "artist") {
      const name = String(e.over?.data.current?.name ?? "");
      setAssign({ track, artist: { id: Number(a), name } });
    } else if (kind === "plrow" && track.from && track.from.playlist === Number(a)) {
      const to = Number(b);
      if (to !== track.from.position)
        movePlaylistItem(track.from.playlist, track.from.position, to).catch(() =>
          notify(t("playlists.failed")),
        );
    }
  };

  const doAssign = (a: Assign) => {
    setAssign(null);
    trackDetails(a.track.id)
      .then((d) =>
        assignTrack(d.id, {
          title: d.title,
          artists: a.artist.name,
          // The track leaves its old album: a single of the new artist.
          album: null,
          trackNo: null,
          year: d.year,
        }),
      )
      .then(() => notify(t("dnd.assigned", { title: a.track.title, artist: a.artist.name })))
      .catch((e) => {
        console.error("assign by drop failed", e);
        notify(t("unknownScreen.failed"));
      });
  };

  return (
    <DndContext
      sensors={sensors}
      // The target is what the pointer is over (a row is as wide as the screen: rectangle
      // overlap would pick a neighbour).
      collisionDetection={pointerWithin}
      onDragStart={onStart}
      onDragOver={onOver}
      onDragEnd={onEnd}
      onDragCancel={() => {
        carried.current = null;
        clearSpring();
        setDragging(null);
      }}
    >
      {children}
      <DragOverlay dropAnimation={null}>
        {dragging && (
          <div className="t-body flex max-w-72 items-center gap-2 rounded-(--radius-md) border border-border bg-bg-flyout px-3 py-2 text-text-primary shadow-(--shadow-flyout)">
            <Music2 size={16} strokeWidth={1.5} className="shrink-0 text-icon" />
            <span className="truncate">{dragging.title}</span>
          </div>
        )}
      </DragOverlay>
      {toPlaylist.dialog}
      {assign && (
        <Dialog
          title={t("dnd.assignTitle")}
          closeLabel={tc("errors.close")}
          onClose={() => setAssign(null)}
        >
          <p className="t-body text-text-primary">
            {t("dnd.assignQuestion", { title: assign.track.title, artist: assign.artist.name })}
          </p>
          <div className="flex flex-wrap gap-3">
            <Button variant="primary" onClick={() => doAssign(assign)}>
              {t("dnd.assign")}
            </Button>
            <Button variant="ghost" onClick={() => setAssign(null)}>
              {t("trackCard.cancel")}
            </Button>
          </div>
        </Dialog>
      )}
    </DndContext>
  );
}
