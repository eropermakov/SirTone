//! Taskbar extensions (T5.2, FR-WIN-004, `07` §4; Microsoft Learn «Taskbar Extensions»,
//! `01b` F-25 — foobar2000, AIMP, MusicBee): ⏮ ⏯ ⏭ under the window's thumbnail, the track's
//! progress on the button (≤ 2 Hz, paused — yellow), a play/pause overlay. Every call goes to the
//! window's own (UI) thread; the buttons are added again whenever Windows says the taskbar
//! button was created (`TaskbarButtonCreated` — also after explorer restarts). Clicks arrive as
//! `WM_COMMAND`/`THBN_CLICKED` through a window subclass and go to one callback.
//! Icons are drawn here (no resource files): white or dark glyphs, 4× supersampled.

use std::sync::OnceLock;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, HGDIOBJ,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::Shell::{
    DefSubclassProc, ITaskbarList3, SetWindowSubclass, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED,
    THB_FLAGS, THB_ICON, THB_TOOLTIP, THBF_ENABLED, THBN_CLICKED, THUMBBUTTON, TaskbarList,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, HICON, ICONINFO, RegisterWindowMessageW, WM_COMMAND,
};
use windows::core::w;

/// The thumbnail buttons, left to right (`iId`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thumb {
    Prev = 0,
    Toggle = 1,
    Next = 2,
}

/// Progress on the taskbar button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    None,
    Playing,
    Paused,
}

type ClickFn = Box<dyn Fn(Thumb) + Send + Sync>;
/// Gets the window whose taskbar button was (re)created — a rebuilt window has a new one.
type CreatedFn = Box<dyn Fn(isize) + Send + Sync>;
static ON_CLICK: OnceLock<ClickFn> = OnceLock::new();
static ON_CREATED: OnceLock<CreatedFn> = OnceLock::new();
static CREATED_MSG: OnceLock<u32> = OnceLock::new();

const SUBCLASS_ID: usize = 0x5172_7442;
/// Drawn at this size; Windows scales to the small icon size of the DPI.
const ICON: usize = 32;

unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_COMMAND && ((wparam.0 >> 16) & 0xffff) as u32 == THBN_CLICKED {
        let thumb = match wparam.0 & 0xffff {
            0 => Some(Thumb::Prev),
            1 => Some(Thumb::Toggle),
            2 => Some(Thumb::Next),
            _ => None,
        };
        if let (Some(t), Some(f)) = (thumb, ON_CLICK.get()) {
            f(t);
        }
        return LRESULT(0);
    }
    if CREATED_MSG.get() == Some(&msg)
        && let Some(f) = ON_CREATED.get()
    {
        f(hwnd.0 as isize);
    }
    // SAFETY: the default handling of a subclassed window, same arguments.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Hooks the window: `on_click` for the buttons, `on_created(hwnd)` when its taskbar button
/// exists (add the buttons then). Call on the window's thread — again for a rebuilt window (the
/// callbacks stay those of the first call; they get the window from the message).
pub fn hook(
    hwnd: isize,
    on_click: impl Fn(Thumb) + Send + Sync + 'static,
    on_created: impl Fn(isize) + Send + Sync + 'static,
) -> bool {
    let _ = ON_CLICK.set(Box::new(on_click));
    let _ = ON_CREATED.set(Box::new(on_created));
    // SAFETY: a static wide string; the message id is process-wide.
    let msg = unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) };
    let _ = CREATED_MSG.set(msg);
    // SAFETY: the window belongs to this thread; the proc and id are static.
    unsafe { SetWindowSubclass(HWND(hwnd as *mut _), Some(subclass), SUBCLASS_ID, 0).as_bool() }
}

/// One glyph as straight 0…1 coverage, 4× supersampled.
fn glyph(t: Thumb, playing: bool) -> Vec<f32> {
    let inside = |x: f32, y: f32| -> bool {
        // Coordinates in 0…1 over the icon.
        let tri_right = |x0: f32, x1: f32, x: f32, y: f32| {
            x >= x0 && x <= x1 && (y - 0.5).abs() <= 0.32 * (x1 - x) / (x1 - x0)
        };
        let tri_left = |x0: f32, x1: f32, x: f32, y: f32| {
            x >= x0 && x <= x1 && (y - 0.5).abs() <= 0.32 * (x - x0) / (x1 - x0)
        };
        let bar = |x0: f32, x1: f32, x: f32, y: f32| x >= x0 && x <= x1 && (y - 0.5).abs() <= 0.32;
        match (t, playing) {
            (Thumb::Toggle, true) => bar(0.28, 0.43, x, y) || bar(0.57, 0.72, x, y),
            (Thumb::Toggle, false) => tri_right(0.3, 0.78, x, y),
            (Thumb::Next, _) => tri_right(0.2, 0.62, x, y) || bar(0.66, 0.78, x, y),
            (Thumb::Prev, _) => bar(0.22, 0.34, x, y) || tri_left(0.38, 0.8, x, y),
        }
    };
    let mut out = vec![0f32; ICON * ICON];
    const S: usize = 4;
    for py in 0..ICON {
        for px in 0..ICON {
            let mut n = 0;
            for sy in 0..S {
                for sx in 0..S {
                    let x = (px as f32 + (sx as f32 + 0.5) / S as f32) / ICON as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / S as f32) / ICON as f32;
                    if inside(x, y) {
                        n += 1;
                    }
                }
            }
            out[py * ICON + px] = n as f32 / (S * S) as f32;
        }
    }
    out
}

/// An icon from coverage: `light` — a white glyph (dark taskbar), else near-black.
fn icon(cov: &[f32], light: bool) -> Option<HICON> {
    let rgb: u32 = if light { 0xff_ff_ff } else { 0x20_20_20 };
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: ICON as i32,
        biHeight: -(ICON as i32), // top-down
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    // SAFETY: a 32-bit top-down DIB of ICON×ICON; `bits` points at its pixels until DeleteObject.
    unsafe {
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let px = std::slice::from_raw_parts_mut(bits.cast::<u32>(), ICON * ICON);
        for (p, c) in px.iter_mut().zip(cov) {
            let a = (c * 255.0).round() as u32;
            // Premultiplied BGRA.
            let ch = |v: u32| v * a / 255;
            *p = (a << 24)
                | (ch((rgb >> 16) & 0xff) << 16)
                | (ch((rgb >> 8) & 0xff) << 8)
                | ch(rgb & 0xff);
        }
        let mask = CreateBitmap(ICON as i32, ICON as i32, 1, 1, None);
        let ii = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let h = CreateIconIndirect(&ii).ok();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        h
    }
}

fn tip(s: &str) -> [u16; 260] {
    let mut out = [0u16; 260];
    for (o, c) in out.iter_mut().zip(s.encode_utf16().take(259)) {
        *o = c;
    }
    out
}

/// The window's taskbar button. Use on the window's (UI) thread only.
pub struct Taskbar {
    list: ITaskbarList3,
    hwnd: HWND,
    /// prev, play, pause, next.
    icons: [HICON; 4],
    /// Small overlays: play, pause.
    overlay: [HICON; 2],
}

impl Taskbar {
    /// `light` — white glyphs (a dark taskbar).
    pub fn new(hwnd: isize, light: bool) -> Option<Self> {
        // SAFETY: COM is up on the UI thread (WebView2 needs it); HrInit before any call.
        let list: ITaskbarList3 =
            unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER).ok()? };
        unsafe { list.HrInit().ok()? };
        let mk = |t, p| icon(&glyph(t, p), light);
        Some(Self {
            hwnd: HWND(hwnd as *mut _),
            icons: [
                mk(Thumb::Prev, false)?,
                mk(Thumb::Toggle, false)?,
                mk(Thumb::Toggle, true)?,
                mk(Thumb::Next, false)?,
            ],
            overlay: [mk(Thumb::Toggle, false)?, mk(Thumb::Toggle, true)?],
            list,
        })
    }

    fn buttons(&self, playing: bool, tips: &[&str; 4]) -> [THUMBBUTTON; 3] {
        let b = |id: u32, icon: HICON, t: &str| THUMBBUTTON {
            dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS,
            iId: id,
            hIcon: icon,
            szTip: tip(t),
            dwFlags: THBF_ENABLED,
            ..Default::default()
        };
        [
            b(Thumb::Prev as u32, self.icons[0], tips[0]),
            if playing {
                b(Thumb::Toggle as u32, self.icons[2], tips[2])
            } else {
                b(Thumb::Toggle as u32, self.icons[1], tips[1])
            },
            b(Thumb::Next as u32, self.icons[3], tips[3]),
        ]
    }

    /// Adds ⏮ ⏯ ⏭ (after `TaskbarButtonCreated`). `tips`: prev, play, pause, next.
    pub fn add(&self, playing: bool, tips: &[&str; 4]) -> bool {
        // SAFETY: our window, three valid buttons.
        unsafe {
            self.list
                .ThumbBarAddButtons(self.hwnd, &self.buttons(playing, tips))
                .is_ok()
        }
    }

    /// Play ↔ pause on the middle button.
    pub fn update(&self, playing: bool, tips: &[&str; 4]) -> bool {
        // SAFETY: as in `add`.
        unsafe {
            self.list
                .ThumbBarUpdateButtons(self.hwnd, &self.buttons(playing, tips))
                .is_ok()
        }
    }

    /// The track's progress on the button (`done`/`total` ms).
    pub fn progress(&self, state: Progress, done: u64, total: u64) {
        let flag = match state {
            Progress::None => TBPF_NOPROGRESS,
            Progress::Playing => TBPF_NORMAL,
            Progress::Paused => TBPF_PAUSED,
        };
        // SAFETY: our window.
        unsafe {
            let _ = self.list.SetProgressState(self.hwnd, flag);
            if state != Progress::None && total > 0 {
                let _ = self
                    .list
                    .SetProgressValue(self.hwnd, done.min(total), total);
            }
        }
    }

    /// ▶/⏸ over the button; `None` — none.
    pub fn overlay(&self, playing: Option<bool>, desc: &str) {
        let wide: Vec<u16> = desc.encode_utf16().chain([0]).collect();
        let icon = match playing {
            Some(true) => self.overlay[0],
            Some(false) => self.overlay[1],
            None => HICON::default(),
        };
        // SAFETY: our window; the description is NUL-terminated and outlives the call.
        unsafe {
            let _ = self
                .list
                .SetOverlayIcon(self.hwnd, icon, windows::core::PCWSTR(wide.as_ptr()));
        }
    }
}

impl Drop for Taskbar {
    fn drop(&mut self) {
        for h in self.icons.iter().chain(&self.overlay) {
            // SAFETY: icons we created.
            let _ = unsafe { DestroyIcon(*h) };
        }
    }
}

/// The Jump List's tasks (T5.2, `07` §4): static entries that start the exe with an argument
/// (`--cmd=toggle`); the running instance gets them through single-instance. `tasks` —
/// `(title, argument)`. Call once at start, on a thread with COM.
pub fn jump_list(exe: &std::path::Path, tasks: &[(String, String)]) -> windows::core::Result<()> {
    use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0, PropVariantClear,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::SHStrDupW;
    use windows::Win32::UI::Shell::{
        DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
    };
    use windows::core::{HSTRING, Interface};
    // SAFETY: COM objects created and used on this thread; strings outlive the calls.
    unsafe {
        let list: ICustomDestinationList =
            CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
        let mut slots = 0u32;
        let _removed: IObjectArray = list.BeginList(&mut slots)?;
        let coll: IObjectCollection =
            CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
        for (title, arg) in tasks {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            link.SetPath(&HSTRING::from(exe.as_os_str()))?;
            link.SetArguments(&HSTRING::from(arg.as_str()))?;
            link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
            let props: IPropertyStore = link.cast()?;
            // A VT_LPWSTR title (CoTaskMem string, freed by PropVariantClear).
            let mut pv = PROPVARIANT {
                Anonymous: PROPVARIANT_0 {
                    Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                        vt: VT_LPWSTR,
                        Anonymous: PROPVARIANT_0_0_0 {
                            pwszVal: SHStrDupW(&HSTRING::from(title.as_str()))?,
                        },
                        ..Default::default()
                    }),
                },
            };
            let r = props.SetValue(&PKEY_Title, &pv);
            let _ = PropVariantClear(&mut pv);
            r?;
            props.Commit()?;
            coll.AddObject(&link)?;
        }
        let arr: IObjectArray = coll.cast()?;
        list.AddUserTasks(&arr)?;
        list.CommitList()
    }
}

/// Windows uses a light taskbar (`SystemUsesLightTheme`): then the glyphs are dark.
pub fn light_taskbar() -> bool {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let mut v = 0u32;
    let mut len = 4u32;
    // SAFETY: a DWORD read into a 4-byte buffer.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut v).cast()),
            Some(&mut len),
        )
    };
    r.is_ok() && v == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_drawn_and_differ() {
        let play = glyph(Thumb::Toggle, false);
        let pause = glyph(Thumb::Toggle, true);
        let cov = |g: &[f32]| g.iter().sum::<f32>() / g.len() as f32;
        for g in [
            &play,
            &pause,
            &glyph(Thumb::Next, false),
            &glyph(Thumb::Prev, false),
        ] {
            let c = cov(g);
            assert!(c > 0.08 && c < 0.5, "{c}");
        }
        assert_ne!(play, pause);
        // Antialiased edges, not a hard mask.
        assert!(play.iter().any(|v| *v > 0.0 && *v < 1.0));
        assert!(icon(&play, true).is_some());
    }
}
