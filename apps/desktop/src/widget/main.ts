/**
 * The desktop widget (T5.1, T5.18, FR-WIN-003/005, `07` §5, D-18): its own small page — no
 * React, ≤ 100 KB of JS — over a transparent window that stays under other windows. Cover,
 * title, ⏮ ⏯ ⏭, «Сон 30 мин» (or the time left) and the focus ring; a click on the cover opens
 * Сиртон; the card is dragged by its background. Texts come from Rust (`widget_texts`), the
 * state from the same events as the main window. Presses and a track change animate with
 * transform/opacity only; `prefers-reduced-motion` turns that off.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "../shared/ui/tokens/sirtone.css";
import "./widget.css";

type Snap = {
  state: string;
  title: string | null;
  artists: string | null;
  cover: string | null;
  hasPrev: boolean;
  hasNext: boolean;
};
type Sleep = { phase: string; remainingMs: number };
type Focus = { phase: string; remainingMs: number; segmentMs: number; paused: boolean };
type Texts = Record<
  | "prev"
  | "play"
  | "pause"
  | "next"
  | "sleep30"
  | "sleepLeft"
  | "nothing"
  | "open"
  | "focusWork"
  | "focusBreak",
  string
>;

const root = document.getElementById("widget") as HTMLDivElement;
const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, parent: HTMLElement) => {
  const e = document.createElement(tag);
  e.className = cls;
  parent.appendChild(e);
  return e;
};

const card = el("div", "w-card", root);
card.dataset.tauriDragRegion = "";
const coverBox = el("button", "w-cover", card);
const cover = el("img", "w-cover-img", coverBox);
cover.alt = "";
cover.draggable = false;
const ring = document.createElementNS("http://www.w3.org/2000/svg", "svg");
ring.setAttribute("viewBox", "0 0 100 100");
ring.classList.add("w-ring");
const ringArc = document.createElementNS("http://www.w3.org/2000/svg", "circle");
ringArc.setAttribute("cx", "50");
ringArc.setAttribute("cy", "50");
ringArc.setAttribute("r", "46");
ring.appendChild(ringArc);
coverBox.appendChild(ring);
const body = el("div", "w-body", card);
body.dataset.tauriDragRegion = "";
const meta = el("div", "w-meta", body);
meta.dataset.tauriDragRegion = "";
const title = el("div", "w-title", meta);
title.dataset.tauriDragRegion = "";
const artists = el("div", "w-artists", meta);
artists.dataset.tauriDragRegion = "";
const row = el("div", "w-row", body);
const prev = el("button", "w-btn", row);
const toggle = el("button", "w-btn w-main", row);
const next = el("button", "w-btn", row);
const sleep = el("button", "w-sleep", row);

const ICON = {
  prev: '<svg viewBox="0 0 24 24"><path d="M6 5h2v14H6zM20 5v14L9 12z"/></svg>',
  next: '<svg viewBox="0 0 24 24"><path d="M16 5h2v14h-2zM4 5v14l11-7z"/></svg>',
  play: '<svg viewBox="0 0 24 24"><path d="M7 4v16l13-8z"/></svg>',
  pause: '<svg viewBox="0 0 24 24"><path d="M6 4h4v16H6zM14 4h4v16h-4z"/></svg>',
};
prev.innerHTML = ICON.prev;
next.innerHTML = ICON.next;
toggle.innerHTML = ICON.play;

let texts: Texts | null = null;
let snap: Snap | null = null;
let sleepState: Sleep = { phase: "off", remainingMs: 0 };
let focus: Focus = { phase: "off", remainingMs: 0, segmentMs: 0, paused: false };

const press = (b: HTMLElement) => {
  b.classList.remove("w-pressed");
  void b.offsetWidth; // restart the animation
  b.classList.add("w-pressed");
};
const call = (b: HTMLElement, cmd: string, args?: Record<string, unknown>) => {
  press(b);
  void invoke(cmd, args).catch(() => {});
};
prev.addEventListener("click", () => call(prev, "playback_prev"));
next.addEventListener("click", () => call(next, "playback_next"));
toggle.addEventListener("click", () => call(toggle, "playback_toggle"));
coverBox.addEventListener("click", () => call(coverBox, "widget_open_main"));
sleep.addEventListener("click", () =>
  sleepState.phase === "off" || sleepState.phase === "ended"
    ? call(sleep, "sleep_start", { kind: "minutes", value: 30 })
    : call(sleep, "sleep_cancel"),
);

let lastTitle: string | null = null;
function render() {
  if (!texts) return;
  const s = snap;
  const playing = s?.state === "playing";
  const loaded = !!s?.title;
  toggle.innerHTML = playing ? ICON.pause : ICON.play;
  toggle.setAttribute("aria-label", playing ? texts.pause : texts.play);
  prev.setAttribute("aria-label", texts.prev);
  next.setAttribute("aria-label", texts.next);
  coverBox.setAttribute("aria-label", texts.open);
  prev.disabled = !s?.hasPrev;
  next.disabled = !s?.hasNext;
  toggle.disabled = !loaded;
  const t = s?.title ?? texts.nothing;
  if (t !== lastTitle) {
    // A new track slides in (FR-WIN-005).
    meta.classList.remove("w-enter");
    void meta.offsetWidth;
    meta.classList.add("w-enter");
    lastTitle = t;
  }
  title.textContent = t;
  artists.textContent = s?.artists ?? "";
  if (s?.cover) {
    cover.src = `http://sirtone-cover.localhost/${s.cover}/160`;
    cover.hidden = false;
  } else {
    cover.hidden = true;
  }
  const sleeping = sleepState.phase !== "off" && sleepState.phase !== "ended";
  sleep.textContent = sleeping
    ? texts.sleepLeft.replace(
        "{min}",
        String(Math.max(1, Math.ceil(sleepState.remainingMs / 60000))),
      )
    : texts.sleep30;
  sleep.classList.toggle("w-on", sleeping);
  // The focus ring: the part of the segment done (T5.18, FR-FOCUS-002).
  const on = focus.phase !== "off" && focus.segmentMs > 0;
  ring.classList.toggle("w-ring-on", on);
  ring.classList.toggle("w-ring-break", focus.phase !== "work");
  if (on) {
    const c = 2 * Math.PI * 46;
    const done = 1 - focus.remainingMs / focus.segmentMs;
    ringArc.style.strokeDasharray = `${c}`;
    ringArc.style.strokeDashoffset = `${c * (1 - Math.min(1, Math.max(0, done)))}`;
    ring.setAttribute("aria-label", focus.phase === "work" ? texts.focusWork : texts.focusBreak);
  }
}

async function start() {
  const st = (await invoke("widget_state")) as { theme: string; accent: string; scale: number };
  document.documentElement.dataset.theme = st.theme;
  document.documentElement.dataset.accent = st.accent;
  texts = (await invoke("widget_texts")) as Texts;
  snap = (await invoke("playback_snapshot")) as Snap;
  sleepState = (await invoke("sleep_status")) as Sleep;
  focus = (await invoke("focus_status")) as Focus;
  render();
  await listen<Snap>("playback-snapshot", (e) => {
    const prevState = snap;
    snap = e.payload;
    if (
      prevState?.state !== snap.state ||
      prevState?.title !== snap.title ||
      prevState?.cover !== snap.cover ||
      prevState?.hasNext !== snap.hasNext ||
      prevState?.hasPrev !== snap.hasPrev
    )
      render();
  });
  await listen<Sleep>("sleep-status", (e) => {
    sleepState = e.payload;
    render();
  });
  await listen<Focus>("focus-status", (e) => {
    focus = e.payload;
    render();
  });
  await listen<{ theme: string; accent: string }>("widget-look", (e) => {
    document.documentElement.dataset.theme = e.payload.theme;
    document.documentElement.dataset.accent = e.payload.accent;
  });
}
void start();
