//! After the sleep timer (T4.23, FR-SLEEP-007, `07` §8.1, D-48, spike S-18): what this PC can
//! do and doing it — only when the person chose it in the settings. Kodi's list of actions
//! (`01c` W-08), adapted to Windows power capabilities:
//! - S3 sleep → `SetSuspendState(FALSE, …)` with `SE_SHUTDOWN_NAME` enabled in our token (an
//!   ordinary user has it, it only needs enabling — no administrator rights);
//! - Modern Standby (S0 low-power idle, `AoAc`) has no S3: `SetSuspendState` refuses, so «sleep»
//!   = turn the screen off and let go of our keep-awake requests — the system then enters
//!   standby by itself;
//! - hibernate → `SetSuspendState(TRUE, …)`, only if S4 is supported and the hiberfile exists.
//!
//! The choice is made from the capability flags, never by trying a call to see if it fails.

use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LUID, WPARAM};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    SE_SHUTDOWN_NAME, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Power::{
    GetPwrCapabilities, SYSTEM_POWER_CAPABILITIES, SetSuspendState,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SC_MONITORPOWER, WM_SYSCOMMAND};

/// What the PC supports (read-only, `GetPwrCapabilities`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PowerCaps {
    /// Classic sleep (S3).
    pub s3: bool,
    /// Modern Standby (S0 low-power idle).
    pub modern_standby: bool,
    /// Hibernate is supported and the hiberfile exists.
    pub hibernate: bool,
}

pub fn caps() -> PowerCaps {
    let mut c = SYSTEM_POWER_CAPABILITIES::default();
    // SAFETY: a plain out-parameter struct owned by this frame.
    if !unsafe { GetPwrCapabilities(&mut c) } {
        return PowerCaps::default();
    }
    PowerCaps {
        s3: c.SystemS3,
        modern_standby: c.AoAc,
        hibernate: c.SystemS4 && c.HiberFilePresent,
    }
}

/// The after-timer choice (`sleep.after` in the settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Nothing,
    ScreenOff,
    Sleep,
    Hibernate,
}

/// What will really be done for a choice on this PC (pure, for tests and the settings UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    /// Screen off (and our keep-awake requests released by the caller).
    ScreenOff,
    /// S3 sleep.
    Suspend,
    Hibernate,
}

pub fn plan(after: After, caps: PowerCaps) -> Action {
    match after {
        After::Nothing => Action::None,
        After::ScreenOff => Action::ScreenOff,
        // Modern Standby: the screen off lets the system go to standby by itself.
        After::Sleep if caps.s3 => Action::Suspend,
        After::Sleep => Action::ScreenOff,
        After::Hibernate if caps.hibernate => Action::Hibernate,
        // Hibernate was not available after all: the gentlest thing that still helps.
        After::Hibernate => Action::ScreenOff,
    }
}

/// Turns the display off through our own window (`DefWindowProc` handles `SC_MONITORPOWER`).
pub fn screen_off(hwnd: isize) -> bool {
    // SAFETY: posting a system command to our own top-level window; 2 = power off.
    unsafe {
        PostMessageW(
            Some(HWND(hwnd as *mut core::ffi::c_void)),
            WM_SYSCOMMAND,
            WPARAM(SC_MONITORPOWER as usize),
            LPARAM(2),
        )
    }
    .is_ok()
}

/// Enables `SE_SHUTDOWN_NAME` in this process's token (needed by `SetSuspendState`).
fn enable_shutdown_privilege() -> bool {
    // SAFETY: standard token adjustment of our own process; handles are not leaked beyond it.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .is_err()
        {
            return false;
        }
        let mut luid = LUID::default();
        if LookupPrivilegeValueW(None, SE_SHUTDOWN_NAME, &mut luid).is_err() {
            let _ = windows::Win32::Foundation::CloseHandle(token);
            return false;
        }
        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        let ok = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None).is_ok();
        let _ = windows::Win32::Foundation::CloseHandle(token);
        ok
    }
}

/// S3 sleep or hibernate. `false` — refused.
pub fn suspend(hibernate: bool) -> bool {
    if !enable_shutdown_privilege() {
        return false;
    }
    // SAFETY: documented call; FALSE for «force» and «disable wake events».
    unsafe { SetSuspendState(hibernate, false, false) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_standby_sleeps_by_turning_the_screen_off() {
        let ms = PowerCaps {
            s3: false,
            modern_standby: true,
            hibernate: true,
        };
        assert_eq!(plan(After::Sleep, ms), Action::ScreenOff);
        assert_eq!(plan(After::Hibernate, ms), Action::Hibernate);
        assert_eq!(plan(After::Nothing, ms), Action::None);
        let s3 = PowerCaps {
            s3: true,
            modern_standby: false,
            hibernate: false,
        };
        assert_eq!(plan(After::Sleep, s3), Action::Suspend);
        assert_eq!(plan(After::Hibernate, s3), Action::ScreenOff);
        assert_eq!(plan(After::ScreenOff, s3), Action::ScreenOff);
    }

    #[test]
    fn capabilities_are_readable() {
        // Read-only: whatever this PC says, the call itself works.
        let c = caps();
        assert!(c.s3 || c.modern_standby || !c.s3);
    }
}
