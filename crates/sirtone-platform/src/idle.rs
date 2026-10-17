//! How long the user has not touched the keyboard or mouse (`GetLastInputInfo`): seamless
//! updates install only while nobody is at the computer (D-76).

use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

/// Milliseconds since the last keyboard/mouse input in this session.
pub fn idle_ms() -> u64 {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a valid, sized LASTINPUTINFO.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0;
    }
    // SAFETY: plain call; both counters wrap the same way.
    u64::from(unsafe { GetTickCount() }.wrapping_sub(info.dwTime))
}

#[cfg(test)]
mod tests {
    #[test]
    fn idle_time_is_sane() {
        assert!(super::idle_ms() < 365 * 24 * 3600 * 1000);
    }
}
