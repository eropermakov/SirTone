// S-05: 100 000 TrackRows (52 px, cover 40) or an album grid (184 px cards, covers 128 px loaded
// at 184) through @tanstack/react-virtual. Covers: 400 WebP files, each row gets its own URL
// (?id=) so every row decodes a fresh image, like `sirtone-cover://` per album.
import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";
import { createRoot } from "react-dom/client";

const N = 100_000;
const params = new URLSearchParams(location.search);
const mode = params.get("mode") ?? "list";
const COVER = Number(params.get("cover") ?? 40);
const words = ["Любовь", "Ночь", "Город", "Звезда", "Highway", "Thunder", "Dream", "Light", "Море", "Ветер"];
const title = (i) => `${words[i % 10]} ${words[(i * 7) % 10].toLowerCase()} ${i}`;
const dir = params.get("dir") ?? "covers";
const cover = (i) => `/${dir}/${String(i % 400).padStart(3, "0")}.webp?id=${Math.floor(i / 12)}`;
const dur = (i) => `${3 + (i % 4)}:${String(i % 60).padStart(2, "0")}`;

function List() {
  const el = useRef(null);
  const v = useVirtualizer({ count: N, getScrollElement: () => el.current, estimateSize: () => 52, overscan: 8 });
  return (
    <div ref={el} id="scroller" style={{ height: "100%", overflow: "auto" }}>
      <div style={{ height: v.getTotalSize(), position: "relative" }}>
        {v.getVirtualItems().map((r) => (
          <div key={r.key} className="row" style={{ transform: `translateY(${r.start}px)` }}>
            <span className="s" style={{ width: 48, textAlign: "right" }}>{r.index + 1}</span>
            <img src={cover(r.index)} width={COVER} height={COVER} loading="lazy" decoding="async" alt="" />
            <div className="t">{title(r.index)}<div className="s">Исполнитель {r.index % 9973}</div></div>
            <div className="t s">Альбом {Math.floor(r.index / 12)}</div>
            <span className="s">{dur(r.index)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function Grid() {
  const el = useRef(null);
  const cols = Math.max(1, Math.floor((window.innerWidth - 24) / (184 + 24)));
  const rows = Math.ceil(N / 12 / cols);
  const v = useVirtualizer({ count: rows, getScrollElement: () => el.current, estimateSize: () => 184 + 56, overscan: 2 });
  return (
    <div ref={el} id="scroller" style={{ height: "100%", overflow: "auto" }}>
      <div style={{ height: v.getTotalSize(), position: "relative" }}>
        {v.getVirtualItems().map((r) =>
          Array.from({ length: cols }, (_, c) => {
            const a = r.index * cols + c;
            return (
              <div key={`${r.key}-${c}`} className="card" style={{ transform: `translate(${12 + c * 208}px, ${r.start}px)` }}>
                <img src={cover(a * 12)} width={184} height={184} loading="lazy" decoding="async" alt="" />
                <div className="t">Альбом {a}</div>
                <div className="t s">Исполнитель {a % 9973}</div>
              </div>
            );
          }),
        )}
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")).render(mode === "grid" ? <Grid /> : <List />);
