//! Bluetooth audio devices (T5.4, FR-WIN-006/007, `07` §6, S-12; `01b` F-27 — ToothTray): the
//! paired Bluetooth audio devices are the audio endpoints whose kernel-streaming filter sits on
//! the Bluetooth bus (`BTHENUM`); «Подключить» / «Отключить» send `KSPROPSETID_BtAudio` ·
//! `KSPROPERTY_ONESHOT_RECONNECT` / `_DISCONNECT` to that filter — what the Windows volume flyout
//! does, no driver is installed or removed (path 2, `BluetoothSetServiceState`, is not used).
//! The last resort is the system page `ms-settings:bluetooth`. A device's endpoints (stereo and
//! hands-free) share its name and are one row. Works next to the system's Bluetooth, never
//! instead of it. Call from a thread that has entered COM.

use std::collections::BTreeMap;

use windows::Win32::Devices::FunctionDiscovery::PKEY_DeviceInterface_FriendlyName;
use windows::Win32::Media::Audio::{
    DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATEMASK_ALL, IDeviceTopology, IMMDevice,
    IMMDeviceEnumerator, IPart, MMDeviceEnumerator, eAll,
};
use windows::Win32::Media::KernelStreaming::{
    IKsControl, KSIDENTIFIER, KSIDENTIFIER_0, KSIDENTIFIER_0_0, KSPROPERTY_ONESHOT_DISCONNECT,
    KSPROPERTY_ONESHOT_RECONNECT, KSPROPERTY_TYPE_GET, KSPROPSETID_BtAudio,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};
use windows::core::{HSTRING, Interface, PWSTR};

/// One paired Bluetooth audio device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtDevice {
    pub name: String,
    pub connected: bool,
    /// Its render endpoint ids (the output picker can choose one).
    pub endpoints: Vec<String>,
}

struct Endpoint {
    id: String,
    name: String,
    active: bool,
    ks: IKsControl,
}

fn take(p: PWSTR) -> String {
    // SAFETY: a CoTaskMem string returned to us; freed here.
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0.cast()));
        s
    }
}

/// The endpoints that sit on Bluetooth, with the filter to talk to.
fn endpoints() -> Vec<Endpoint> {
    let mut out = Vec::new();
    // SAFETY: COM calls on interfaces obtained here; each failure skips one endpoint.
    unsafe {
        let Ok(en) =
            CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
        else {
            return out;
        };
        let Ok(list) = en.EnumAudioEndpoints(eAll, DEVICE_STATE(DEVICE_STATEMASK_ALL)) else {
            return out;
        };
        for i in 0..list.GetCount().unwrap_or(0) {
            let Ok(dev) = list.Item(i) else { continue };
            let Some(ks) = bt_filter(&en, &dev) else {
                continue;
            };
            let id = dev.GetId().map(take).unwrap_or_default();
            let name = dev
                .OpenPropertyStore(STGM_READ)
                .and_then(|p| p.GetValue(&PKEY_DeviceInterface_FriendlyName))
                .and_then(|v| PropVariantToStringAlloc(&v))
                .map(take)
                .unwrap_or_default();
            let active = dev.GetState().is_ok_and(|s| s == DEVICE_STATE_ACTIVE);
            if !name.is_empty() {
                out.push(Endpoint {
                    id,
                    name,
                    active,
                    ks,
                });
            }
        }
    }
    out
}

/// The kernel-streaming filter behind an endpoint when it is a Bluetooth one (ToothTray).
unsafe fn bt_filter(en: &IMMDeviceEnumerator, dev: &IMMDevice) -> Option<IKsControl> {
    // SAFETY: the caller's COM objects; the chain endpoint → topology → connector → filter.
    unsafe {
        let topo: IDeviceTopology = dev.Activate(CLSCTX_ALL, None).ok()?;
        let conn = topo.GetConnector(0).ok()?;
        let other = conn.GetConnectedTo().ok()?;
        let part: IPart = other.cast().ok()?;
        let filter_topo = part.GetTopologyObject().ok()?;
        let filter_id = take(filter_topo.GetDeviceId().ok()?);
        if !filter_id.to_ascii_lowercase().contains("bthenum") {
            return None;
        }
        let filter = en.GetDevice(&HSTRING::from(filter_id)).ok()?;
        filter.Activate::<IKsControl>(CLSCTX_ALL, None).ok()
    }
}

/// The paired Bluetooth audio devices, one row per device.
pub fn devices() -> Vec<BtDevice> {
    let mut by: BTreeMap<String, BtDevice> = BTreeMap::new();
    for e in endpoints() {
        let d = by.entry(e.name.clone()).or_insert_with(|| BtDevice {
            name: e.name.clone(),
            connected: false,
            endpoints: Vec::new(),
        });
        d.connected |= e.active;
        d.endpoints.push(e.id);
    }
    by.into_values().collect()
}

/// «Подключить» (`true`) / «Отключить»: the one-shot property to every endpoint of the device.
/// `false` — it has no Bluetooth endpoint here or the driver refused.
pub fn set_connected(name: &str, connect: bool) -> bool {
    let id = if connect {
        KSPROPERTY_ONESHOT_RECONNECT.0
    } else {
        KSPROPERTY_ONESHOT_DISCONNECT.0
    };
    let prop = KSIDENTIFIER {
        Anonymous: KSIDENTIFIER_0 {
            Anonymous: KSIDENTIFIER_0_0 {
                Set: KSPROPSETID_BtAudio,
                Id: id as u32,
                Flags: KSPROPERTY_TYPE_GET,
            },
        },
    };
    let mut any = false;
    for e in endpoints().into_iter().filter(|e| e.name == name) {
        let mut ret = 0u32;
        // SAFETY: a property request without data, as the volume flyout sends it.
        let ok = unsafe {
            e.ks.KsProperty(
                &prop,
                std::mem::size_of::<KSIDENTIFIER>() as u32,
                std::ptr::null_mut(),
                0,
                &mut ret,
            )
            .is_ok()
        };
        any |= ok;
    }
    any
}

/// The last resort: Windows' own Bluetooth page.
pub fn open_settings() {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::w;
    // SAFETY: static wide strings.
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            w!("ms-settings:bluetooth"),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_never_fails_and_touches_nothing() {
        crate::sessions::com_init();
        // Whatever is paired here: a list (maybe empty), no connect or disconnect.
        let list = devices();
        assert!(
            list.iter()
                .all(|d| !d.name.is_empty() && !d.endpoints.is_empty())
        );
    }
}
