//! Hub «Телефоны» (stage 8, FR-HUB-001, D-139): what phones the PC sees. Install, renewal and
//! transfer arrive with M2/M6; the UI already shows the cards and the wizard's first steps.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PhonePlatform {
    Ios,
    Android,
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PhoneState {
    Ready,
    NeedsTrust,
    NotSupportedYet,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PhoneInfo {
    pub id: String,
    pub platform: PhonePlatform,
    pub usb: bool,
    pub name: Option<String>,
    pub model: Option<String>,
    pub os_version: Option<String>,
    pub state: PhoneState,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PhonesStatus {
    pub phones: Vec<PhoneInfo>,
    /// Windows sees iPhones only with Apple's component (wizard step 2).
    pub apple_service: bool,
}

/// Phones connected now. Never fails: a missing Apple service is a state, not an error.
#[tauri::command(async)]
#[specta::specta]
pub async fn phones_list() -> PhonesStatus {
    use sirtone_devicelink as dl;
    let (phones, errors) = dl::all_phones().await;
    PhonesStatus {
        apple_service: !errors
            .iter()
            .any(|e| matches!(e, dl::LinkError::AppleServiceMissing(_))),
        phones: phones
            .into_iter()
            .map(|p| PhoneInfo {
                id: p.id,
                platform: match p.platform {
                    dl::Platform::Ios => PhonePlatform::Ios,
                    dl::Platform::Android => PhonePlatform::Android,
                },
                usb: p.link == dl::Link::Usb,
                name: p.name,
                model: p.model,
                os_version: p.os_version,
                state: match p.state {
                    dl::PhoneState::Ready => PhoneState::Ready,
                    dl::PhoneState::NeedsTrust => PhoneState::NeedsTrust,
                    dl::PhoneState::NotSupportedYet => PhoneState::NotSupportedYet,
                },
            })
            .collect(),
    }
}
