//! Window caption tinted to the app theme (S-20 variant (a), D-60): the native Windows title bar
//! keeps the real caption buttons and Snap Layouts, DWM paints it in `bg-base` with `text-primary`
//! text (Windows 11 build 22000+; older builds ignore the colour attributes and keep dark/light).

use std::ffi::c_void;

use windows::Win32::Foundation::{COLORREF, HWND};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
    DWMWINDOWATTRIBUTE, DwmSetWindowAttribute,
};

/// `DESIGN.md` §3 tokens used for the caption, as 0xRRGGBB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptionColors {
    pub background: u32,
    pub text: u32,
}

pub const DARK: CaptionColors = CaptionColors {
    background: 0x333333,
    text: 0xf5f5f5,
};
/// Owner palettes (D-74): «Оранжевая» and «Чёрно-белая».
pub const EMBER: CaptionColors = CaptionColors {
    background: 0x333333,
    text: 0xf5f5f5,
};
pub const MONO: CaptionColors = CaptionColors {
    background: 0x333333,
    text: 0xf5f5f5,
};
pub const LIGHT: CaptionColors = CaptionColors {
    background: 0xf2eee6,
    text: 0x1c1616,
};

/// 0xRRGGBB → COLORREF (0x00BBGGRR).
pub fn colorref(rgb: u32) -> u32 {
    ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff)
}

fn set<T>(hwnd: HWND, attr: DWMWINDOWATTRIBUTE, value: &T) -> bool {
    // SAFETY: `value` points to a live `T` of the size we pass; DWM copies it.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attr,
            std::ptr::from_ref(value).cast::<c_void>(),
            size_of::<T>() as u32,
        )
    }
    .is_ok()
}

/// Applies dark/light caption to a top-level window. `hwnd` is the raw window handle.
/// Returns whether the colour attributes were accepted (false on Windows 10).
pub fn apply(hwnd: isize, dark: bool) -> bool {
    apply_colors(hwnd, dark, if dark { DARK } else { LIGHT })
}

/// Caption in the given colours (`dark` selects the dark caption buttons).
pub fn apply_colors(hwnd: isize, dark: bool, c: CaptionColors) -> bool {
    let hwnd = HWND(hwnd as *mut c_void);
    let _ = set(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &i32::from(dark));
    let bg = COLORREF(colorref(c.background));
    let caption = set(hwnd, DWMWA_CAPTION_COLOR, &bg);
    let _ = set(hwnd, DWMWA_BORDER_COLOR, &bg);
    let text = set(hwnd, DWMWA_TEXT_COLOR, &COLORREF(colorref(c.text)));
    caption && text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_swaps_red_and_blue() {
        assert_eq!(colorref(0x161111), 0x111116);
        assert_eq!(colorref(0xf2eee6), 0xe6eef2);
    }
}
