//! iPhone through the Apple Mobile Device service (usbmuxd; on Windows `127.0.0.1:27015`).

use async_trait::async_trait;
use idevice::IdeviceService;
use idevice::lockdown::LockdownClient;
use idevice::provider::IdeviceProvider;
use idevice::usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdConnection, UsbmuxdDevice};

use crate::{Link, LinkError, Phone, PhoneInstaller, PhoneState, Platform};

const LABEL: &str = "sirtone";

pub struct IosInstaller;

fn link(d: &UsbmuxdDevice) -> Link {
    match d.connection_type {
        Connection::Usb => Link::Usb,
        _ => Link::Network,
    }
}

async fn describe(d: &UsbmuxdDevice) -> Phone {
    let mut phone = Phone {
        id: d.udid.clone(),
        platform: Platform::Ios,
        link: link(d),
        name: None,
        model: None,
        os_version: None,
        state: PhoneState::NeedsTrust,
    };
    let provider = d.to_provider(UsbmuxdAddr::default(), LABEL);
    let Ok(mut lockdown) = LockdownClient::connect(&provider).await else {
        return phone;
    };
    // Model and iOS answer without a session; the name needs the pairing (trust) record.
    phone.model = lockdown
        .get_value(Some("ProductType"), None)
        .await
        .ok()
        .and_then(|v| v.into_string());
    phone.os_version = lockdown
        .get_value(Some("ProductVersion"), None)
        .await
        .ok()
        .and_then(|v| v.into_string());
    if let Ok(pairing) = provider.get_pairing_file().await
        && lockdown.start_session(&pairing).await.is_ok()
    {
        phone.name = lockdown
            .get_value(Some("DeviceName"), None)
            .await
            .ok()
            .and_then(|v| v.into_string());
        phone.state = PhoneState::Ready;
    }
    phone
}

#[async_trait]
impl PhoneInstaller for IosInstaller {
    fn platform(&self) -> Platform {
        Platform::Ios
    }

    async fn phones(&self) -> Result<Vec<Phone>, LinkError> {
        let mut mux = UsbmuxdConnection::default()
            .await
            .map_err(|e| LinkError::AppleServiceMissing(format!("{e:?}")))?;
        let devices = mux
            .get_devices()
            .await
            .map_err(|e| LinkError::Device(format!("{e:?}")))?;
        let mut out = Vec::with_capacity(devices.len());
        for d in &devices {
            out.push(describe(d).await);
        }
        // Phones with Wi-Fi access (П4б) the Apple service does not see over the network.
        for s in crate::wifi::seen_recently().await {
            if !out.iter().any(|p| p.id == s.udid) {
                out.push(Phone {
                    id: s.udid,
                    platform: Platform::Ios,
                    link: Link::Network,
                    name: None,
                    model: None,
                    os_version: None,
                    state: PhoneState::Ready,
                });
            }
        }
        // The same phone over cable and Wi-Fi: keep the cable.
        out.sort_by_key(|p| (p.id.clone(), p.link != Link::Usb));
        out.dedup_by(|a, b| a.id == b.id);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without iTunes the service is missing; with it and no phone the list is empty. Either way
    /// the call answers instead of hanging (the wizard relies on that).
    #[tokio::test]
    async fn answers_without_a_phone() {
        let r = tokio::time::timeout(std::time::Duration::from_secs(10), IosInstaller.phones())
            .await
            .expect("usbmuxd query must not hang");
        if let Err(e) = r {
            assert_eq!(e.code(), "phone.apple_service_missing");
        }
    }
}
