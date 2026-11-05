//! The hub's link to phones (PC only; D-139, `docs/mobile/README.md` §6, §10).
//!
//! [`PhoneInstaller`] is one trait for both platforms: [`ios::IosInstaller`] talks to iPhones
//! through the Apple Mobile Device service (installed with iTunes) using `idevice`;
//! [`android::AndroidInstaller`] is a stub for now («Версия для Android готовится», MAT-18).
//! Signing and installing (`isideload`, profiles via `misagent`) arrive with M2 after check П4
//! on a real phone. Errors carry stable codes; the UI writes the words (MR §6.3). Secrets
//! (Apple ID tokens) never reach logs.

pub mod android;
pub mod ios;

use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Ios,
    Android,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Usb,
    Network,
}

/// A phone the hub sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phone {
    /// UDID (iPhone) or serial (Android).
    pub id: String,
    pub platform: Platform,
    pub link: Link,
    /// The phone's own name, once the phone trusts this PC.
    pub name: Option<String>,
    /// `iPhone16,1` …
    pub model: Option<String>,
    pub os_version: Option<String>,
    pub state: PhoneState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneState {
    /// Paired: the hub may read details, install and renew.
    Ready,
    /// The phone must tap «Доверять» and enter its code (wizard step 3).
    NeedsTrust,
    /// Recognised, but this platform's app is not available yet (Android, MAT-18).
    NotSupportedYet,
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    /// Windows has no Apple Mobile Device service: wizard step 2 (install the Apple component).
    #[error("apple mobile device service unavailable: {0}")]
    AppleServiceMissing(String),
    #[error("device: {0}")]
    Device(String),
    /// Not built yet (M2): signing and install.
    #[error("not implemented yet")]
    NotYet,
}

impl LinkError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::AppleServiceMissing(_) => "phone.apple_service_missing",
            Self::Device(_) => "phone.device",
            Self::NotYet => "phone.not_yet",
        }
    }
}

#[async_trait]
pub trait PhoneInstaller: Send + Sync {
    fn platform(&self) -> Platform;
    /// Phones connected now (cable, and Wi-Fi sync for iPhones).
    async fn phones(&self) -> Result<Vec<Phone>, LinkError>;
    /// Installs (or updates) the app from a verified package (sha256 + channel signature).
    async fn install(&self, _phone: &Phone, _package: &std::path::Path) -> Result<(), LinkError> {
        Err(LinkError::NotYet)
    }
    /// Renews the signature (iPhone: new profiles only, no reinstall).
    async fn renew(&self, _phone: &Phone) -> Result<(), LinkError> {
        Err(LinkError::NotYet)
    }
}

/// Every installer the hub knows, iPhone first.
pub fn installers() -> Vec<Box<dyn PhoneInstaller>> {
    vec![
        Box::new(ios::IosInstaller),
        Box::new(android::AndroidInstaller),
    ]
}

/// All phones of all platforms, with the error of each platform that could not answer (the
/// wizard turns `phone.apple_service_missing` into step 2).
pub async fn all_phones() -> (Vec<Phone>, Vec<LinkError>) {
    let (mut out, mut errors) = (Vec::new(), Vec::new());
    for i in installers() {
        match i.phones().await {
            Ok(p) => out.extend(p),
            Err(e) => {
                tracing::debug!(target: "sirtone::devicelink", code = e.code(), "phones");
                errors.push(e);
            }
        }
    }
    (out, errors)
}
