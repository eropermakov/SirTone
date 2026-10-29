//! The single-instance lock stays put (FR-SHELL-004, D-124). The single-instance plugin finds the
//! running app by a hidden, top-level, «visible» message window. With the main window hidden in
//! the tray, that window is what Windows and .NET take for the app's main window, and a plain
//! `WM_CLOSE` (Task Manager «Снять задачу», installers, window tools) destroys it. The next launch
//! then finds no window and starts a second player on the same library. Here the window ignores
//! `WM_CLOSE`; only the app's own exit removes it.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId, WM_CLOSE};
use windows::core::HSTRING;

const SUBCLASS_ID: usize = 0x5349_4e53; // «SINS»

unsafe extern "system" fn subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_CLOSE {
        return LRESULT(0);
    }
    // SAFETY: the default chain for a window we subclassed.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Guards the plugin's window of app `id` (its config identifier). Call on the thread that made
/// it (the main thread, after the plugin's setup). `false` — not found or not ours.
pub fn guard_lock_window(id: &str) -> bool {
    let class = HSTRING::from(format!("{id}-sic"));
    let name = HSTRING::from(format!("{id}-siw"));
    // SAFETY: wide strings live through the call; a missing window is a null handle.
    let Ok(hwnd) = (unsafe { FindWindowW(&class, &name) }) else {
        return false;
    };
    let mut pid = 0;
    // SAFETY: a window handle just found; pid is a plain out value.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != std::process::id() {
        return false;
    }
    // SAFETY: the window belongs to this process and thread; the proc and id are static.
    unsafe { SetWindowSubclass(hwnd, Some(subclass), SUBCLASS_ID, 0).as_bool() }
}
