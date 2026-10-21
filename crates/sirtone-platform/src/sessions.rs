//! Other applications' sound (T4.14, FR-PLAY-011, `04` §7 — copy of the `wasapi` crate's
//! «processes» example, `01b` F-04): the audio sessions of the default render endpoint through
//! Core Audio (`IAudioSessionManager2`), each with its process, state and peak meter. Sirtone's
//! own process tree (the app and its WebView2 children) and the system-sounds session are not
//! «foreign». Call from a thread that has entered COM (MTA).

use std::collections::{HashMap, HashSet};

use windows::Win32::Foundation::{CloseHandle, S_OK};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    AudioSessionStateActive, IAudioSessionControl, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eCommunications, eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::core::Interface;

/// One sounding session of another application.
#[derive(Debug, Clone, PartialEq)]
pub struct Foreign {
    pub pid: u32,
    /// The executable's file name, lower case (`chrome.exe`).
    pub exe: String,
    /// Peak 0…1 (linear) right now.
    pub peak: f32,
}

/// Enters COM for the calling thread (multithreaded apartment).
pub fn com_init() {
    // SAFETY: plain call; a second call on the same thread is harmless.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

/// Every process with its parent and executable name.
fn processes() -> HashMap<u32, (u32, String)> {
    let mut out = HashMap::new();
    // SAFETY: a snapshot handle closed below; the entry struct is sized as required.
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let len = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&e.szExeFile[..len]).to_lowercase();
                out.insert(e.th32ProcessID, (e.th32ParentProcessID, name));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// The foreground window's application — its exe file name, lower case (T5.6, FR-CTX-003).
/// Window titles are never read.
pub fn foreground_exe() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain calls; a null window gives pid 0.
    let pid = unsafe {
        let hwnd = GetForegroundWindow();
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid
    };
    if pid == 0 {
        return None;
    }
    processes().remove(&pid).map(|(_, exe)| exe)
}

/// Sirtone's process and everything it started (WebView2 and its helpers).
pub fn own_tree() -> HashSet<u32> {
    let me = std::process::id();
    let all = processes();
    let mut own: HashSet<u32> = HashSet::from([me]);
    loop {
        let before = own.len();
        for (pid, (parent, _)) in &all {
            if own.contains(parent) {
                own.insert(*pid);
            }
        }
        if own.len() == before {
            return own;
        }
    }
}

/// The active, foreign sessions on the default output and their peaks.
pub fn foreign(own: &HashSet<u32>) -> Vec<Foreign> {
    let mut out = Vec::new();
    let names = processes();
    // SAFETY: COM calls on interfaces obtained here; every failure just yields fewer sessions.
    unsafe {
        let Ok(en) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return out;
        };
        let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eConsole) else {
            return out;
        };
        let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
            return out;
        };
        let Ok(list) = mgr.GetSessionEnumerator() else {
            return out;
        };
        let n = list.GetCount().unwrap_or(0);
        for i in 0..n {
            let Ok(ctl) = list.GetSession(i) else {
                continue;
            };
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else {
                continue;
            };
            if ctl2.IsSystemSoundsSession() == S_OK {
                continue;
            }
            let Ok(pid) = ctl2.GetProcessId() else {
                continue;
            };
            if pid == 0 || own.contains(&pid) {
                continue;
            }
            if !matches!(IAudioSessionControl::GetState(&ctl), Ok(s) if s == AudioSessionStateActive)
            {
                continue;
            }
            let peak = ctl
                .cast::<IAudioMeterInformation>()
                .ok()
                .and_then(|m| m.GetPeakValue().ok())
                .unwrap_or(0.0);
            out.push(Foreign {
                pid,
                exe: names.get(&pid).map(|n| n.1.clone()).unwrap_or_default(),
                peak,
            });
        }
    }
    out
}

/// Executable names (lower case) of the processes with an active session on the default
/// communications microphone (T5.18, D-49): a call app that is capturing is in a call.
pub fn capturing() -> HashSet<String> {
    let mut out = HashSet::new();
    let names = processes();
    // SAFETY: COM calls on interfaces obtained here; a failure yields fewer names.
    unsafe {
        let Ok(en) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return out;
        };
        let Ok(dev) = en.GetDefaultAudioEndpoint(eCapture, eCommunications) else {
            return out;
        };
        let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
            return out;
        };
        let Ok(list) = mgr.GetSessionEnumerator() else {
            return out;
        };
        for i in 0..list.GetCount().unwrap_or(0) {
            let Ok(ctl) = list.GetSession(i) else {
                continue;
            };
            if !matches!(IAudioSessionControl::GetState(&ctl), Ok(s) if s == AudioSessionStateActive)
            {
                continue;
            }
            let Ok(pid) = ctl
                .cast::<IAudioSessionControl2>()
                .and_then(|c| c.GetProcessId())
            else {
                continue;
            };
            if let Some((_, exe)) = names.get(&pid) {
                out.insert(exe.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_tree_holds_us_and_nothing_foreign_crashes() {
        com_init();
        let own = own_tree();
        assert!(own.contains(&std::process::id()));
        // Whatever is playing on this machine, the call returns (no panic, no hang).
        let f = foreign(&own);
        assert!(f.iter().all(|s| !own.contains(&s.pid)));
    }
}
