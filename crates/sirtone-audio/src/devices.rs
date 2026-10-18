//! Output device notifications (T1.6, FR-PLAY-012, docs/04 §3): `IMMNotificationClient` through
//! the `wasapi` crate. Callbacks run on a Windows audio thread and only forward commands to the
//! engine; all decisions (follow the new default, pause on loss) are made on the engine thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use wasapi::{DeviceEnumerator, DeviceEventCallbacks, DeviceState, Direction, Role};

use crate::engine::{EngineCmd, EngineHandle};

/// Keeps the notification registration alive until `stop` is set.
pub fn watch(engine: EngineHandle, stop: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("sirtone-devices".into())
        .spawn(move || {
            let _ = wasapi::initialize_mta().ok();
            let Ok(enumerator) = DeviceEnumerator::new() else {
                return;
            };
            let mut cb = DeviceEventCallbacks::new();
            let e = engine.clone();
            cb.set_default_device_callback(move |direction, role, _id| {
                if matches!(direction, Direction::Render) && matches!(role, Role::Console) {
                    e.send(EngineCmd::DefaultDeviceChanged);
                }
            });
            let e = engine.clone();
            cb.set_device_state_callback(move |id, state| {
                if !matches!(state, DeviceState::Active) {
                    e.send(EngineCmd::DeviceLost(id));
                }
            });
            let e = engine.clone();
            cb.set_device_removed_callback(move |id| e.send(EngineCmd::DeviceLost(id)));
            let Ok(_registration) = enumerator.register_notification_callback(cb) else {
                tracing::warn!(target: "sirtone::audio", "device notifications unavailable");
                return;
            };
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .ok();
}

/// Whether the endpoint `id` still exists and is active (unplugged headphones are not).
pub fn is_active(id: &str) -> bool {
    let _ = wasapi::initialize_mta().ok();
    DeviceEnumerator::new()
        .and_then(|e| e.get_device(id))
        .and_then(|d| d.get_state())
        .is_ok_and(|s| matches!(s, DeviceState::Active))
}
