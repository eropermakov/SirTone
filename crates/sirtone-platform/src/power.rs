//! Background work that never gets in the way (T3.10, FR-PERF-005, docs/06 §6.2): is the PC on
//! battery, is a full-screen game or presentation running, and a below-normal, EcoQoS thread.

use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadInformation, SetThreadPriority,
    THREAD_POWER_THROTTLING_CURRENT_VERSION, THREAD_POWER_THROTTLING_EXECUTION_SPEED,
    THREAD_POWER_THROTTLING_STATE, THREAD_PRIORITY_BELOW_NORMAL, ThreadPowerThrottling,
};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};

/// Running on battery (AC line offline).
pub fn on_battery() -> bool {
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: `s` is a valid out-structure.
    unsafe { GetSystemPowerStatus(&mut s) }.is_ok() && s.ACLineStatus == 0
}

/// A full-screen game (D3D), a presentation or a «busy» full-screen app is in front.
pub fn fullscreen_busy() -> bool {
    // SAFETY: plain query.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => {
            state == QUNS_BUSY
                || state == QUNS_RUNNING_D3D_FULL_SCREEN
                || state == QUNS_PRESENTATION_MODE
        }
        Err(_) => false,
    }
}

/// The calling thread becomes below-normal priority with EcoQoS (efficiency cores, low clock).
pub fn background_thread() {
    // SAFETY: the pseudo-handle of the current thread; failures leave the defaults.
    unsafe {
        let me = GetCurrentThread();
        let _ = SetThreadPriority(me, THREAD_PRIORITY_BELOW_NORMAL);
        let state = THREAD_POWER_THROTTLING_STATE {
            Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
        };
        let _ = SetThreadInformation(
            me,
            ThreadPowerThrottling,
            std::ptr::from_ref(&state).cast(),
            std::mem::size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn queries_answer() {
        let _ = super::on_battery();
        let _ = super::fullscreen_busy();
        super::background_thread();
    }
}
