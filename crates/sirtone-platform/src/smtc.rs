//! System Media Transport Controls (T1.23, FR-WIN-002, docs/07 §3): media keys, the Windows 11
//! media flyout, the lock screen and third-party widgets. The fallback path of docs/07 §3: the
//! `windows` crate directly (`ISystemMediaTransportControlsInterop::GetForWindow`) on a hidden
//! service window owned by its own thread — independent of the WebView, so it keeps working when
//! the main window is hidden or its WebView is destroyed (D-52).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::StorageFile;
use windows::Storage::Streams::RandomAccessStreamReference;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
    RegisterClassW, TranslateMessage, WINDOW_EX_STYLE, WNDCLASSW, WS_OVERLAPPED,
};
use windows::core::{HSTRING, w};

/// A button pressed in the flyout, on the keyboard (media keys) or by a widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtcButton {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    /// Seek requested from the timeline, milliseconds.
    Seek(u64),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SmtcTrack {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// JPEG/PNG file for the thumbnail (docs/07 §3: 512 px in the cache).
    pub cover: Option<PathBuf>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtcState {
    Playing,
    Paused,
    Stopped,
}

enum Cmd {
    Track(Option<SmtcTrack>),
    State {
        state: SmtcState,
        position_ms: u64,
        has_next: bool,
        has_prev: bool,
    },
    Quit,
}

/// Handle to the SMTC thread. Dropping it closes the session.
pub struct Smtc {
    tx: Sender<Cmd>,
}

impl Smtc {
    /// Starts the service window and registers with SMTC; `None` if Windows refuses (then media
    /// keys simply do not reach Сиртон — nothing else breaks).
    pub fn start(on_button: impl Fn(SmtcButton) + Send + Sync + 'static) -> Option<Self> {
        let (tx, rx) = channel();
        let (ready_tx, ready_rx) = channel();
        let on_button: Arc<dyn Fn(SmtcButton) + Send + Sync> = Arc::new(on_button);
        std::thread::Builder::new()
            .name("sirtone-smtc".into())
            .spawn(move || run(&rx, on_button, &ready_tx))
            .ok()?;
        ready_rx
            .recv_timeout(Duration::from_secs(5))
            .ok()
            .filter(|ok| *ok)
            .map(|_| Self { tx })
    }

    /// The track now playing (`None` clears the flyout).
    pub fn set_track(&self, track: Option<SmtcTrack>) {
        let _ = self.tx.send(Cmd::Track(track));
    }

    /// Play state and position; the timeline is refreshed at most once a second unless the state
    /// changes (docs/07 §3).
    pub fn set_state(&self, state: SmtcState, position_ms: u64, has_next: bool, has_prev: bool) {
        let _ = self.tx.send(Cmd::State {
            state,
            position_ms,
            has_next,
            has_prev,
        });
    }
}

impl Drop for Smtc {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: default handling for our own hidden window.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

/// A top-level window that is never shown: SMTC needs an HWND (message-only windows do not do).
fn service_window() -> windows::core::Result<HWND> {
    // SAFETY: plain Win32 window creation with static, NUL-terminated names.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = w!("SirToneSmtcWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            w!("Сиртон"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
    }
}

fn span(ms: u64) -> TimeSpan {
    TimeSpan {
        Duration: (ms as i64).saturating_mul(10_000),
    }
}

fn button(b: SystemMediaTransportControlsButton) -> Option<SmtcButton> {
    Some(match b {
        SystemMediaTransportControlsButton::Play => SmtcButton::Play,
        SystemMediaTransportControlsButton::Pause => SmtcButton::Pause,
        SystemMediaTransportControlsButton::Next => SmtcButton::Next,
        SystemMediaTransportControlsButton::Previous => SmtcButton::Previous,
        SystemMediaTransportControlsButton::Stop => SmtcButton::Stop,
        _ => return None,
    })
}

struct Session {
    smtc: SystemMediaTransportControls,
    duration_ms: u64,
    last_state: Option<SmtcState>,
    last_timeline: Instant,
}

impl Session {
    fn open(
        hwnd: HWND,
        on_button: &Arc<dyn Fn(SmtcButton) + Send + Sync>,
    ) -> windows::core::Result<Self> {
        let interop = windows::core::factory::<
            SystemMediaTransportControls,
            ISystemMediaTransportControlsInterop,
        >()?;
        // SAFETY: `hwnd` is our live service window.
        let smtc: SystemMediaTransportControls = unsafe { interop.GetForWindow(hwnd)? };
        smtc.SetIsEnabled(true)?;
        smtc.SetIsPlayEnabled(true)?;
        smtc.SetIsPauseEnabled(true)?;
        smtc.SetIsStopEnabled(true)?;
        smtc.SetIsNextEnabled(false)?;
        smtc.SetIsPreviousEnabled(false)?;
        smtc.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
        let cb = on_button.clone();
        smtc.ButtonPressed(&TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(move |_, args| {
            if let Some(args) = args.as_ref()
                && let Some(b) = button(args.Button()?)
            {
                cb(b);
            }
            Ok(())
        }))?;
        let cb = on_button.clone();
        smtc.PlaybackPositionChangeRequested(&TypedEventHandler::<
            SystemMediaTransportControls,
            PlaybackPositionChangeRequestedEventArgs,
        >::new(move |_, args| {
            if let Some(args) = args.as_ref() {
                let d = args.RequestedPlaybackPosition()?.Duration.max(0);
                cb(SmtcButton::Seek((d / 10_000) as u64));
            }
            Ok(())
        }))?;
        Ok(Self {
            smtc,
            duration_ms: 0,
            last_state: None,
            last_timeline: Instant::now(),
        })
    }

    fn track(&mut self, t: Option<SmtcTrack>) -> windows::core::Result<()> {
        let du = self.smtc.DisplayUpdater()?;
        du.ClearAll()?;
        let Some(t) = t else {
            self.smtc.SetPlaybackStatus(MediaPlaybackStatus::Closed)?;
            self.duration_ms = 0;
            return du.Update();
        };
        du.SetType(MediaPlaybackType::Music)?;
        let mp = du.MusicProperties()?;
        mp.SetTitle(&HSTRING::from(t.title.as_str()))?;
        mp.SetArtist(&HSTRING::from(t.artist.as_str()))?;
        mp.SetAlbumTitle(&HSTRING::from(t.album.as_str()))?;
        if let Some(cover) = t.cover.as_ref()
            && let Ok(file) = StorageFile::GetFileFromPathAsync(&HSTRING::from(cover.as_os_str()))
                .and_then(|op| op.join())
            && let Ok(stream) = RandomAccessStreamReference::CreateFromFile(&file)
        {
            du.SetThumbnail(&stream)?;
        }
        du.Update()?;
        self.duration_ms = t.duration_ms;
        self.last_state = None; // force a timeline refresh for the new track
        Ok(())
    }

    fn state(
        &mut self,
        s: SmtcState,
        pos: u64,
        next: bool,
        prev: bool,
    ) -> windows::core::Result<()> {
        let changed = self.last_state != Some(s);
        if changed {
            self.smtc.SetPlaybackStatus(match s {
                SmtcState::Playing => MediaPlaybackStatus::Playing,
                SmtcState::Paused => MediaPlaybackStatus::Paused,
                SmtcState::Stopped => MediaPlaybackStatus::Stopped,
            })?;
            self.smtc.SetIsNextEnabled(next)?;
            self.smtc
                .SetIsPreviousEnabled(prev || s != SmtcState::Stopped)?;
            self.last_state = Some(s);
        }
        if changed || self.last_timeline.elapsed() >= Duration::from_secs(1) {
            let tl = SystemMediaTransportControlsTimelineProperties::new()?;
            tl.SetStartTime(span(0))?;
            tl.SetMinSeekTime(span(0))?;
            tl.SetEndTime(span(self.duration_ms))?;
            tl.SetMaxSeekTime(span(self.duration_ms))?;
            tl.SetPosition(span(pos.min(self.duration_ms.max(pos))))?;
            self.smtc.UpdateTimelineProperties(&tl)?;
            self.last_timeline = Instant::now();
        }
        Ok(())
    }
}

fn run(rx: &Receiver<Cmd>, on_button: Arc<dyn Fn(SmtcButton) + Send + Sync>, ready: &Sender<bool>) {
    // SAFETY: this thread owns an STA with a message pump below.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let session =
        service_window().and_then(|hwnd| Session::open(hwnd, &on_button).map(|s| (hwnd, s)));
    let (hwnd, mut session) = match session {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(target: "sirtone::smtc", error = %e, "SMTC unavailable");
            let _ = ready.send(false);
            return;
        }
    };
    let _ = ready.send(true);
    let mut msg = MSG::default();
    loop {
        // SAFETY: standard message pump for this thread's windows.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        match rx.recv_timeout(Duration::from_millis(30)) {
            Ok(Cmd::Track(t)) => {
                if let Err(e) = session.track(t) {
                    tracing::warn!(target: "sirtone::smtc", error = %e, "metadata not shown");
                }
            }
            Ok(Cmd::State {
                state,
                position_ms,
                has_next,
                has_prev,
            }) => {
                let _ = session.state(state, position_ms, has_next, has_prev);
            }
            Ok(Cmd::Quit) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    let _ = session.smtc.SetIsEnabled(false);
    // SAFETY: our own window, destroyed on its owning thread.
    let _ = unsafe { DestroyWindow(hwnd) };
}

/// One media session as Windows sees it (verification: `sirtone-cli smtc`).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub app: String,
    pub title: String,
    pub artist: String,
    pub status: String,
}

/// Every media session Windows knows about, read the way the flyout and widgets read them.
pub fn sessions() -> windows::core::Result<Vec<SessionInfo>> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Manager;
    let manager = Manager::RequestAsync()?.join()?;
    let mut out = Vec::new();
    for s in manager.GetSessions()? {
        let props = s.TryGetMediaPropertiesAsync()?.join()?;
        let status = s
            .GetPlaybackInfo()
            .and_then(|i| i.PlaybackStatus())
            .map(|st| format!("{st:?}"))
            .unwrap_or_default();
        out.push(SessionInfo {
            app: s.SourceAppUserModelId()?.to_string(),
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
            status,
        });
    }
    Ok(out)
}

/// Presses ⏯ on the session whose title contains `title` — what the flyout button does (smoke
/// check of the button path; the owner's own session is never matched by a test title).
pub fn toggle_session(title: &str) -> windows::core::Result<bool> {
    use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager as Manager;
    let manager = Manager::RequestAsync()?.join()?;
    for s in manager.GetSessions()? {
        let props = s.TryGetMediaPropertiesAsync()?.join()?;
        if props.Title()?.to_string().contains(title) {
            return s.TryTogglePlayPauseAsync()?.join();
        }
    }
    Ok(false)
}
