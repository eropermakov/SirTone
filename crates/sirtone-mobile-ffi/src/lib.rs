//! The phone apps' door into the shared Rust core (D-137, `docs/mobile/README.md` §8).
//!
//! The composition root of the phone, as `apps/desktop/src-tauri` is for the PC: it wires domain
//! crates together and exposes a small, stable surface to Swift (iPhone) and Kotlin (Android)
//! through UniFFI. Calls never run on the UI thread (the apps call it off the main actor); the
//! surface grows stage by stage (M3 player, M4 library, M5 wave, M6 transfer).

uniffi::setup_scaffolding!();

pub mod library;
pub mod wave;

/// Errors crossing the bridge: a stable code, the app builds the text (project rule).
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("{code}")]
    Failed { code: String },
}

/// What the core is: version and build target, shown by «О программе» and used by check П1.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoreInfo {
    pub version: String,
    pub target: String,
}

/// П1: the SwiftUI screen shows this — proof the Rust core runs on the phone.
#[uniffi::export]
pub fn core_info() -> CoreInfo {
    CoreInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
    }
}

/// The search/recognition text key (`docs/05` §1.1) — the same folding on every platform.
#[uniffi::export]
pub fn norm_text(text: String) -> Option<String> {
    sirtone_core::text::norm(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_and_norm() {
        let i = core_info();
        assert_eq!(i.version, env!("CARGO_PKG_VERSION"));
        assert!(!i.target.is_empty());
        assert_eq!(
            norm_text("Ёлка — Live".into()).as_deref(),
            Some("елка live")
        );
        assert_eq!(norm_text("  ".into()), None);
    }
}
