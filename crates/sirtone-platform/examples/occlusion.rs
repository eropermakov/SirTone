//! Prints how much of a window is covered and whether it is minimised (T4.6 diagnostics):
//! `cargo run -p sirtone-platform --example occlusion -- <hwnd>`; lists the windows above it.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GW_HWNDPREV, GWL_EXSTYLE, GetClassNameW, GetWindow, GetWindowLongPtrW, GetWindowRect,
    GetWindowTextW, IsWindowVisible,
};

fn main() {
    let h: isize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(0);
    println!(
        "hwnd {h}: minimised {}, covered {:.2}",
        sirtone_platform::occlusion::minimized(h),
        sirtone_platform::occlusion::covered(h)
    );
    unsafe {
        let mut w = GetWindow(HWND(h as *mut _), GW_HWNDPREV);
        while let Ok(x) = w {
            if x.is_invalid() {
                break;
            }
            if IsWindowVisible(x).as_bool() {
                let mut r = RECT::default();
                let _ = GetWindowRect(x, &mut r);
                let mut cls = [0u16; 128];
                let n = GetClassNameW(x, &mut cls) as usize;
                let mut t = [0u16; 128];
                let m = GetWindowTextW(x, &mut t) as usize;
                let ex = GetWindowLongPtrW(x, GWL_EXSTYLE) as u32;
                println!(
                    "  above {:?} class {} title {:?} rect {},{} {}x{} ex {ex:#x}",
                    x.0,
                    String::from_utf16_lossy(&cls[..n]),
                    String::from_utf16_lossy(&t[..m]),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top
                );
            }
            w = GetWindow(x, GW_HWNDPREV);
        }
    }
}
