//! Android: a stub (MAT-18). The app and the ADB installer come with the Android stage
//! (`docs/mobile/android.md`); detection over ADB waits for the driver decision (Q-25, Q-27).

use async_trait::async_trait;

use crate::{LinkError, Phone, PhoneInstaller, Platform};

pub struct AndroidInstaller;

#[async_trait]
impl PhoneInstaller for AndroidInstaller {
    fn platform(&self) -> Platform {
        Platform::Android
    }

    async fn phones(&self) -> Result<Vec<Phone>, LinkError> {
        Ok(Vec::new())
    }
}
