//! Is the window seen at all (T4.6, FR-VIZ-008, `06` §7.6): minimised, or covered by other
//! windows — Lively's rule, `01a` §B.3: pause when ≥ 95 % of the window is under others. Coverage
//! is sampled on a 16 × 16 grid of the window against the visible, uncloaked, non-transparent
//! top-level windows above it in Z order.

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::UI::WindowsAndMessaging::{
    GW_HWNDPREV, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, GetWindowRect, IsIconic,
    IsWindowVisible, WS_EX_TRANSPARENT,
};

const GRID: i32 = 16;

fn hwnd(h: isize) -> HWND {
    HWND(h as *mut core::ffi::c_void)
}

/// The window is minimised.
pub fn minimized(h: isize) -> bool {
    // SAFETY: plain query; an invalid handle answers false.
    unsafe { IsIconic(hwnd(h)) }.as_bool()
}

fn cloaked(h: HWND) -> bool {
    let mut v = 0u32;
    // SAFETY: `v` is a valid 4-byte out-buffer.
    unsafe {
        DwmGetWindowAttribute(
            h,
            DWMWA_CLOAKED,
            (&mut v as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    }
    .is_ok()
        && v != 0
}

fn rect(h: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    // SAFETY: `r` is a valid out-structure.
    unsafe { GetWindowRect(h, &mut r) }.ok()?;
    (r.right > r.left && r.bottom > r.top).then_some(r)
}

/// The share (0…1) of the window under other windows above it.
pub fn covered(h: isize) -> f32 {
    let me = hwnd(h);
    let Some(r) = rect(me) else { return 0.0 };
    let mut above = Vec::new();
    // SAFETY: walking the Z order with plain queries; a stale handle ends the walk.
    unsafe {
        let mut w = GetWindow(me, GW_HWNDPREV);
        while let Ok(x) = w {
            if x.is_invalid() {
                break;
            }
            let ex = GetWindowLongPtrW(x, GWL_EXSTYLE) as u32;
            if IsWindowVisible(x).as_bool()
                && !IsIconic(x).as_bool()
                && ex & WS_EX_TRANSPARENT.0 == 0
                && !cloaked(x)
                && let Some(o) = rect(x)
            {
                above.push(o);
            }
            w = GetWindow(x, GW_HWNDPREV);
        }
    }
    coverage(&r, &above)
}

/// Grid sampling of `r` against `above`.
fn coverage(r: &RECT, above: &[RECT]) -> f32 {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    if w <= 0 || h <= 0 {
        return 0.0;
    }
    let mut hit = 0;
    for i in 0..GRID {
        for j in 0..GRID {
            let x = r.left + (2 * i + 1) * w / (2 * GRID);
            let y = r.top + (2 * j + 1) * h / (2 * GRID);
            if above
                .iter()
                .any(|o| x >= o.left && x < o.right && y >= o.top && y < o.bottom)
            {
                hit += 1;
            }
        }
    }
    hit as f32 / (GRID * GRID) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn coverage_counts_the_covered_share() {
        let me = r(0, 0, 1600, 900);
        assert_eq!(coverage(&me, &[]), 0.0);
        assert_eq!(coverage(&me, &[r(-10, -10, 2000, 1000)]), 1.0);
        let half = coverage(&me, &[r(0, 0, 800, 900)]);
        assert!((half - 0.5).abs() < 0.01, "{half}");
        // Two windows side by side cover it all.
        assert_eq!(
            coverage(&me, &[r(0, 0, 800, 900), r(800, 0, 1600, 900)]),
            1.0
        );
        // A bad handle: nothing known — not covered, not minimised.
        assert_eq!(covered(0), 0.0);
        assert!(!minimized(0));
    }
}
