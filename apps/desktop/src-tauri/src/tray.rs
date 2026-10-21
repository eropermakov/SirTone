//! Tray (T0.10, T1.21, FR-SHELL-003/006, docs/07 §1): the window's close button hides to the tray,
//! music keeps playing; «Выход» is the only full exit. Menu: «Открыть», «Играет: …», ⏯, ⏮, ⏭,
//! «Выход». Labels are read in Rust from the UI string files and every action goes straight to the
//! Rust playback service, so the tray works even when the WebView is gone (D-52).

use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::playback::Playback;

const RU_SHELL: &str = include_str!("../../../../assets/i18n/ru/shell.json");

#[derive(Debug, Clone)]
struct Labels {
    open: String,
    now_playing: String,
    nothing: String,
    play: String,
    pause: String,
    prev: String,
    next: String,
    quit: String,
    tooltip: String,
    sleep_start: String,
    sleep_left: String,
    sleep_extend: String,
    focus_start: String,
    focus_on: String,
    focus_work: String,
    focus_break: String,
    event_none: String,
    event_offer: String,
    /// i18n event names (`events.name.<id>`).
    event_names: std::collections::BTreeMap<String, String>,
}

fn labels() -> Labels {
    let doc: serde_json::Value = serde_json::from_str(RU_SHELL).unwrap_or_default();
    let get = |k: &str, fallback: &str| {
        doc["tray"][k]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback)
            .to_owned()
    };
    Labels {
        open: get("open", "Open"),
        now_playing: get("nowPlaying", "Playing: {title}"),
        nothing: get("nothing", "Nothing is playing"),
        play: get("play", "Play"),
        pause: get("pause", "Pause"),
        prev: get("prev", "Previous"),
        next: get("next", "Next"),
        quit: get("quit", "Quit"),
        tooltip: get("tooltip", "SirTone"),
        sleep_start: get("sleepStart", "Sleep 30 min"),
        sleep_left: get("sleepLeft", "Sleep: {min} min left — cancel"),
        sleep_extend: get("sleepExtend", "+15 min"),
        focus_start: get("focusStart", "Focus 25/5"),
        focus_on: get("focusOn", "Focus: {phase}, {min} min — stop"),
        focus_work: get("focusWork", "work"),
        focus_break: get("focusBreak", "break"),
        event_none: get("eventNone", "No event wave now"),
        event_offer: get("eventOffer", "{event}: play the event wave"),
        event_names: doc["events"]["name"]
            .as_object()
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Menu items that change with playback.
struct Items {
    now: MenuItem<Wry>,
    play_pause: MenuItem<Wry>,
    prev: MenuItem<Wry>,
    next: MenuItem<Wry>,
    sleep: MenuItem<Wry>,
    focus: MenuItem<Wry>,
    event: MenuItem<Wry>,
    labels: Labels,
    /// Last shown state, so unchanged snapshots do not touch the native menu.
    last: Option<TrayState>,
}

/// Native menu calls wait for the main thread (tauri `run_item_main_thread!`), so they never run
/// on the caller's thread: updates go to the `sirtone-tray` thread, latest wins.
#[derive(Default)]
pub struct TrayMenu {
    items: Mutex<Option<Items>>,
    tx: Mutex<Option<Sender<Msg>>>,
}

enum Msg {
    Play(TrayState),
    /// The sleep timer, focus and event lines (T4.21, T4.24, T5.14, FR-SHELL-006).
    Modes(String, String, Option<String>),
}

/// The sleep line: start, the minutes left (a click cancels), or «+15 мин» while woken.
fn sleep_text(l: &Labels, s: &crate::sleep::SleepStatus) -> String {
    match s.phase.as_str() {
        "off" | "ended" => l.sleep_start.clone(),
        "woken" => l.sleep_extend.clone(),
        _ => l
            .sleep_left
            .replace("{min}", &s.remaining_ms.div_ceil(60_000).to_string()),
    }
}

/// The focus line: start, or the phase and minutes left (a click stops).
fn focus_text(l: &Labels, s: &crate::focus::FocusStatus) -> String {
    let phase = match s.phase.as_str() {
        "off" => return l.focus_start.clone(),
        "work" => &l.focus_work,
        _ => &l.focus_break,
    };
    l.focus_on
        .replace("{phase}", phase)
        .replace("{min}", &s.remaining_ms.div_ceil(60_000).to_string())
}

/// The sleep timer or focus changed: the tray lines follow (never blocks).
pub fn refresh_modes(app: &AppHandle) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };
    let texts = {
        let items = menu.items.lock().unwrap_or_else(|p| p.into_inner());
        let Some(items) = items.as_ref() else { return };
        let sleep = app
            .try_state::<crate::sleep::Sleep>()
            .map(|s| sleep_text(&items.labels, &s.status()))
            .unwrap_or_else(|| items.labels.sleep_start.clone());
        let focus = app
            .try_state::<crate::focus::Focus>()
            .map(|f| focus_text(&items.labels, &f.status()))
            .unwrap_or_else(|| items.labels.focus_start.clone());
        let event = crate::events::offer().id.map(|id| {
            let name = items.labels.event_names.get(&id).cloned().unwrap_or(id);
            items.labels.event_offer.replace("{event}", &name)
        });
        (sleep, focus, event)
    };
    if let Some(tx) = menu.tx.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        let _ = tx.send(Msg::Modes(texts.0, texts.1, texts.2));
    }
}

/// What the tray shows about playback.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrayState {
    pub title: Option<String>,
    pub artists: Option<String>,
    pub playing: bool,
    pub has_prev: bool,
    pub has_next: bool,
}

/// Menu line and tooltip for the current track (pure: tested).
fn texts(l: &Labels, s: &TrayState) -> (String, String) {
    match &s.title {
        Some(t) => {
            let line = match &s.artists {
                Some(a) if !a.is_empty() => format!("{t} — {a}"),
                _ => t.clone(),
            };
            // Windows truncates long tray tips at 127 chars.
            let short: String = line.chars().take(60).collect();
            (
                l.now_playing.replace("{title}", &short),
                format!("{} — {short}", l.tooltip),
            )
        }
        None => (l.nothing.clone(), l.tooltip.clone()),
    }
}

/// Reflects playback in the tray (called from the engine's event sink): never blocks.
pub fn update(app: &AppHandle, s: &TrayState) {
    if let Some(menu) = app.try_state::<TrayMenu>()
        && let Some(tx) = menu.tx.lock().unwrap_or_else(|p| p.into_inner()).as_ref()
    {
        let _ = tx.send(Msg::Play(s.clone()));
    }
}

fn apply(app: &AppHandle, s: &TrayState) {
    let Some(menu) = app.try_state::<TrayMenu>() else {
        return;
    };
    let mut guard = menu.items.lock().unwrap_or_else(|p| p.into_inner());
    let Some(items) = guard.as_mut() else { return };
    if items.last.as_ref() == Some(s) {
        return;
    }
    let (line, tip) = texts(&items.labels, s);
    let _ = items.now.set_text(line);
    let _ = items.play_pause.set_text(if s.playing {
        &items.labels.pause
    } else {
        &items.labels.play
    });
    let _ = items.play_pause.set_enabled(s.title.is_some());
    let _ = items.prev.set_enabled(s.has_prev || s.title.is_some());
    let _ = items.next.set_enabled(s.has_next);
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tip));
    }
    items.last = Some(s.clone());
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn toggle_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_main(app);
        }
    }
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let pb = app.try_state::<Playback>();
    match (event.id().as_ref(), pb) {
        ("open", _) | ("now", _) => show_main(app),
        ("play_pause", Some(pb)) => pb.toggle(),
        ("prev", Some(pb)) => pb.prev(),
        ("next", Some(pb)) => pb.next(),
        ("event", _) => {
            if let Some(id) = crate::events::offer().id {
                let _ = crate::events::event_answer(app.clone(), id, "accept".into());
            }
        }
        ("focus", _) => {
            if let Some(f) = app.try_state::<crate::focus::Focus>() {
                f.tray_action();
            }
        }
        ("sleep", _) => {
            if let Some(s) = app.try_state::<crate::sleep::Sleep>() {
                s.tray_action();
            }
        }
        ("quit", _) => app.exit(0),
        _ => {}
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<TrayIcon<Wry>> {
    let l = labels();
    let now = MenuItem::with_id(app, "now", &l.nothing, true, None::<&str>)?;
    let play_pause = MenuItem::with_id(app, "play_pause", &l.play, false, None::<&str>)?;
    let prev = MenuItem::with_id(app, "prev", &l.prev, false, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", &l.next, false, None::<&str>)?;
    let sleep = MenuItem::with_id(app, "sleep", &l.sleep_start, true, None::<&str>)?;
    let focus = MenuItem::with_id(app, "focus", &l.focus_start, true, None::<&str>)?;
    let event = MenuItem::with_id(app, "event", &l.event_none, false, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", &l.open, true, None::<&str>)?,
            &now,
            &PredefinedMenuItem::separator(app)?,
            &play_pause,
            &prev,
            &next,
            &PredefinedMenuItem::separator(app)?,
            &sleep,
            &focus,
            &event,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", &l.quit, true, None::<&str>)?,
        ],
    )?;
    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(&l.tooltip)
        .on_menu_event(on_menu)
        .on_tray_icon_event(|tray, event| {
            // Left click opens/hides the window (Spotify/Discord); right click shows the menu.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    let state = app.state::<TrayMenu>();
    *state.items.lock().unwrap_or_else(|p| p.into_inner()) = Some(Items {
        now,
        play_pause,
        prev,
        next,
        sleep,
        focus,
        event,
        labels: l,
        last: None,
    });
    let (tx, rx) = channel::<Msg>();
    let handle = app.clone();
    std::thread::Builder::new()
        .name("sirtone-tray".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                // Latest wins, separately for playback and the sleep line.
                let (mut play, mut sleep) = (None, None);
                for m in std::iter::once(first).chain(rx.try_iter()) {
                    match m {
                        Msg::Play(s) => play = Some(s),
                        Msg::Modes(s, f, e) => sleep = Some((s, f, e)),
                    }
                }
                if let Some(s) = play {
                    apply(&handle, &s);
                }
                if let Some((s, f, e)) = sleep
                    && let Some(menu) = handle.try_state::<TrayMenu>()
                    && let Some(items) = menu
                        .items
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .as_ref()
                {
                    let _ = items.sleep.set_text(s);
                    let _ = items.focus.set_text(f);
                    let _ = items.event.set_enabled(e.is_some());
                    let _ = items
                        .event
                        .set_text(e.unwrap_or_else(|| items.labels.event_none.clone()));
                }
            }
        })
        .ok();
    *state.tx.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    Ok(tray)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_labels_come_from_ru_strings() {
        let l = labels();
        assert_eq!(l.quit, "Выход");
        assert_eq!(l.pause, "Пауза");
        assert!(l.now_playing.contains("{title}"));
    }

    #[test]
    fn now_playing_line_and_tooltip() {
        let l = labels();
        let s = TrayState {
            title: Some("Город".into()),
            artists: Some("Кино".into()),
            playing: true,
            ..TrayState::default()
        };
        let (line, tip) = texts(&l, &s);
        assert_eq!(line, "Играет: Город — Кино");
        assert_eq!(tip, "Сиртон — Город — Кино");
        let (line, tip) = texts(&l, &TrayState::default());
        assert_eq!(line, l.nothing);
        assert_eq!(tip, "Сиртон");
        let long = TrayState {
            title: Some("я".repeat(200)),
            ..TrayState::default()
        };
        assert!(texts(&l, &long).1.chars().count() < 127);
    }
}
