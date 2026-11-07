//! IPC error model (docs/03 §5.5). Stable codes; **texts are produced by the UI** from `errors.<code>`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum ErrorCode {
    FileNotFound,
    UnsupportedFormat,
    DecodeFailed,
    PathTooLong,
    DiskFull,
    PermissionDenied,
    DeviceUnavailable,
    DeviceBusy,
    NetworkOffline,
    RateLimited,
    RemoteError,
    DbBusy,
    DbCorrupt,
    Cancelled,
    InvalidArgument,
    LibraryOffline,
    /// No library folder chosen yet (docs/03 §8.2): the UI asks for one.
    LibraryRootNotSet,
    NotSupported,
    InvalidConfig,
    /// Stage 8 hub (D-132, D-139): the Apple ID login is missing or ended — the wizard asks again.
    AppleSignedOut,
    /// The Apple ID login failed (password, code, account).
    AppleLogin,
    /// Signing or installing on the phone failed.
    PhoneInstall,
    /// No app package to install (dev: `SIRTONE_IOS_APP`; later GitHub Releases).
    PhonePackageMissing,
    /// Moving the library to or from the phone failed (M6.2); a repeat resumes.
    PhoneTransfer,
    /// The phone has no room for the music (MR §6.1 step 4).
    PhoneNoSpace,
    /// The iPhone is locked: unlock it and go on (MR §6.3).
    PhoneLocked,
    /// A free Apple ID runs 3 such apps on a phone at once (device error 0xE8008021).
    PhoneAppLimit,
    /// A free Apple ID registers 10 new App IDs in 7 days; a new install waits (renewals do not).
    PhoneAppIdLimit,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type, thiserror::Error)]
#[error("{code:?}")]
pub struct AppError {
    pub code: ErrorCode,
    pub retryable: bool,
    /// For logs/diagnostics, never shown to the user as-is.
    pub details: Option<String>,
}

impl AppError {
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            retryable: false,
            details: None,
        }
    }

    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    pub fn with_details(mut self, details: impl std::fmt::Display) -> Self {
        self.details = Some(details.to_string());
        self
    }
}
