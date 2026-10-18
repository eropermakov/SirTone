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
