//! The owner's Apple ID for installing on iPhone (D-132, D-139, D-143; check П4).
//!
//! - **Login** once: the password and the 2FA code come from the caller (the hub's form, or the
//!   CLI's console) and are dropped right after; only the developer-services token (`xcode.auth`,
//!   about a year), the adsid, the anisette state and the signing key are kept — in Windows
//!   Credential Manager, one service, keys prefixed per Apple ID.
//! - **Install / reinstall** without the password: a session from the stored token, `isideload`
//!   registers the phone, the App ID and the certificate (never revoking one), signs (our
//!   `apple-codesign` shim) and installs over usbmuxd.
//! - **Forget**: deletes every key of that Apple ID («Отключить телефон»).
//!
//! Network: Apple's login and developer services and the anisette server (`08` §4) — the caller
//! checks the offline master switch first (this crate cannot see settings). Nothing secret
//! reaches errors or logs: errors keep the first line of a report only (its attachments may carry
//! Apple plists), and callers cap `isideload` at WARN and `apple_codesign` at ERROR.

use std::path::{Path, PathBuf};

use idevice::usbmuxd::{UsbmuxdAddr, UsbmuxdConnection};
use isideload::anisette::remote_v3::RemoteV3AnisetteProvider;
use isideload::auth::apple_account::{
    AppToken, AppleAccount, TwoFactorCallbackParams, TwoFactorCallbackResponse,
};
use isideload::dev::developer_session::DeveloperSession;
use isideload::sideload::builder::MaxCertsBehavior;
use isideload::sideload::{SideloaderBuilder, TeamSelection};
use isideload::util::callbacks::{ExtensionsBehaviorCallbackBox, MaxCertsCallbackBox};
use isideload::util::keyring_storage::KeyringStorage;
use isideload::util::storage::SideloadingStorage;
use sha2::{Digest, Sha256};

use crate::LinkError;

/// Credential Manager service for every Apple ID the hub knows.
const SERVICE: &str = "studio.serkov.sirtone.apple";
/// The spike's service (`spikes/S-P4-isideload`): its identity is adopted once, so the hub keeps
/// using the certificate it made instead of asking Apple for another (free accounts have few).
pub const SPIKE_SERVICE: &str = "sirtone-p4";
/// The certificate's machine name at Apple («SirTone-PC»): finds our key's certificate again.
const MACHINE: &str = "SirTone-PC";
/// Anisette servers (D-133, Q-26): tried in order. One for now — more after П4б.
const ANISETTE: &[&str] = &["https://ani.iloader.app"];

fn sha_hex(s: &str) -> String {
    let d = Sha256::digest(s.trim().to_lowercase().as_bytes());
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Keys of one Apple ID inside the shared service: `<account>:<key>`.
struct AccountStorage {
    inner: KeyringStorage,
    prefix: String,
}

impl AccountStorage {
    fn new(service: &str, apple_id: &str) -> Self {
        Self {
            inner: KeyringStorage::new(service.to_string()),
            prefix: sha_hex(apple_id)[..16].to_string(),
        }
    }
    fn k(&self, key: &str) -> String {
        format!("{}:{key}", self.prefix)
    }
}

impl SideloadingStorage for AccountStorage {
    fn store(&self, key: &str, value: &str) -> Result<(), rootcause::Report> {
        self.inner.store(&self.k(key), value)
    }
    fn retrieve(&self, key: &str) -> Result<Option<String>, rootcause::Report> {
        self.inner.retrieve(&self.k(key))
    }
    fn store_data(&self, key: &str, value: &[u8]) -> Result<(), rootcause::Report> {
        self.inner.store_data(&self.k(key), value)
    }
    fn retrieve_data(&self, key: &str) -> Result<Option<Vec<u8>>, rootcause::Report> {
        self.inner.retrieve_data(&self.k(key))
    }
    fn delete(&self, key: &str) -> Result<(), rootcause::Report> {
        self.inner.delete(&self.k(key))
    }
}

// Our own keys, and the ones isideload writes (anisette state, the signing key by e-mail hash).
const TOKEN: &str = "token";
const TOKEN_EXPIRY: &str = "token_expiry";
const TOKEN_DURATION: &str = "token_duration";
const ADSID: &str = "adsid";
const ANISETTE_STATE: &str = isideload::anisette::remote_v3::ANISETTE_STATE_KEY;
fn key_name(apple_id: &str) -> String {
    // isideload: sha256 of the e-mail as given, hex, + "/key".
    let d = Sha256::digest(apple_id.as_bytes());
    let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
    format!("{hex}/key")
}

/// The report's first context line («● …»): its message, never its attachments (D-132).
fn short(e: &rootcause::Report) -> String {
    e.to_string()
        .lines()
        .find_map(|l| l.trim_start().strip_prefix('●'))
        .unwrap_or("")
        .trim()
        .to_string()
}

fn anisette(storage: AccountStorage) -> Result<RemoteV3AnisetteProvider, LinkError> {
    Ok(RemoteV3AnisetteProvider::default()
        .map_err(|e| LinkError::Apple("phone.apple_anisette", short(&e)))?
        .set_url(ANISETTE[0])
        .set_storage(Box::new(storage))
        .set_serial_number("2".to_string()))
}

/// What the 2FA step asks the person (the UI words it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TwoFactorAsk {
    /// The code went by SMS (else to the person's Apple devices).
    pub sms: bool,
    /// The previous code or method did not work.
    pub retry: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TwoFactorAnswer {
    Code(String),
    SendToDevices,
    Resend,
    Abort,
}

/// The stored login of an Apple ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedIn {
    /// When the developer-services token ends (unix ms): then the person logs in again.
    pub token_expiry_ms: u64,
}

/// Logs in once and keeps the token; `password` is dropped by the caller right after.
pub async fn login<F, Fut>(apple_id: &str, password: &str, ask: F) -> Result<SignedIn, LinkError>
where
    F: Fn(TwoFactorAsk) -> Fut + Send + Sync,
    Fut: Future<Output = TwoFactorAnswer> + Send,
{
    let ask = &ask;
    let two_factor = move |p: TwoFactorCallbackParams| async move {
        let a = ask(TwoFactorAsk {
            sms: p.sms,
            retry: p.unknown,
        })
        .await;
        Ok::<_, rootcause::Report>(match a {
            TwoFactorAnswer::Code(c) => TwoFactorCallbackResponse::SubmitCode(c),
            TwoFactorAnswer::SendToDevices => TwoFactorCallbackResponse::SendToDevices,
            TwoFactorAnswer::Resend => TwoFactorCallbackResponse::ResendCode,
            TwoFactorAnswer::Abort => TwoFactorCallbackResponse::Abort,
        })
    };
    let mut account = AppleAccount::builder(apple_id)
        .anisette_provider(anisette(AccountStorage::new(SERVICE, apple_id))?)
        .login(password, two_factor)
        .await
        .map_err(|e| LinkError::Apple("phone.apple_login", short(&e)))?;
    let token = account
        .get_app_token("xcode.auth")
        .await
        .map_err(|e| LinkError::Apple("phone.apple_login", short(&e)))?;
    let adsid = account
        .spd
        .as_ref()
        .and_then(|d| d.get("adsid"))
        .and_then(|v| v.as_string())
        .ok_or(LinkError::Apple("phone.apple_login", "adsid".into()))?
        .to_string();
    let s = AccountStorage::new(SERVICE, apple_id);
    let put = |k: &str, v: &str| {
        s.store(k, v)
            .map_err(|e| LinkError::Apple("phone.apple_store", short(&e)))
    };
    put(TOKEN, &token.token)?;
    put(TOKEN_EXPIRY, &token.expiry.to_string())?;
    put(TOKEN_DURATION, &token.duration.to_string())?;
    put(ADSID, &adsid)?;
    Ok(SignedIn {
        token_expiry_ms: token.expiry,
    })
}

fn stored(s: &AccountStorage, k: &str) -> Option<String> {
    s.retrieve(k).ok().flatten().filter(|v| !v.is_empty())
}

/// The stored login, if any (no network).
pub fn signed_in(apple_id: &str) -> Option<SignedIn> {
    let s = AccountStorage::new(SERVICE, apple_id);
    stored(&s, TOKEN)?;
    Some(SignedIn {
        token_expiry_ms: stored(&s, TOKEN_EXPIRY)?.parse().ok()?,
    })
}

/// Adopts the identity the П4 spike made (token, anisette device, signing key), once: the hub
/// then reuses the same Apple certificate. Returns whether anything was copied.
pub fn adopt(from_service: &str, apple_id: &str) -> Result<bool, LinkError> {
    let to = AccountStorage::new(SERVICE, apple_id);
    if stored(&to, TOKEN).is_some() {
        return Ok(false);
    }
    let from = KeyringStorage::new(from_service.to_string());
    let err = |e: rootcause::Report| LinkError::Apple("phone.apple_store", short(&e));
    let mut copied = false;
    // The spike kept its token keys as `<apple-id>:<key>` and isideload's keys bare.
    for k in [TOKEN, TOKEN_EXPIRY, TOKEN_DURATION, ADSID] {
        if let Some(v) = from.retrieve(&format!("{apple_id}:{k}")).map_err(err)? {
            to.store(k, &v).map_err(err)?;
            copied = true;
        }
    }
    for k in [ANISETTE_STATE.to_string(), key_name(apple_id)] {
        if let Some(v) = from.retrieve_data(&k).map_err(err)? {
            to.store_data(&k, &v).map_err(err)?;
            copied = true;
        }
    }
    Ok(copied)
}

/// Deletes everything kept for this Apple ID («Отключить телефон», D-132).
pub fn forget(apple_id: &str) -> Result<(), LinkError> {
    let s = AccountStorage::new(SERVICE, apple_id);
    for k in [
        TOKEN.to_string(),
        TOKEN_EXPIRY.to_string(),
        TOKEN_DURATION.to_string(),
        ADSID.to_string(),
        ANISETTE_STATE.to_string(),
        key_name(apple_id),
    ] {
        s.delete(&k)
            .map_err(|e| LinkError::Apple("phone.apple_store", short(&e)))?;
    }
    Ok(())
}

async fn session(apple_id: &str) -> Result<DeveloperSession, LinkError> {
    let s = AccountStorage::new(SERVICE, apple_id);
    let need =
        |k: &str| stored(&s, k).ok_or(LinkError::Apple("phone.apple_signed_out", String::new()));
    let token = AppToken {
        token: need(TOKEN)?,
        expiry: need(TOKEN_EXPIRY)?.parse().unwrap_or(0),
        duration: need(TOKEN_DURATION)?.parse().unwrap_or(0),
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    if token.expiry != 0 && token.expiry <= now_ms {
        return Err(LinkError::Apple(
            "phone.apple_signed_out",
            "token expired".into(),
        ));
    }
    let adsid = need(ADSID)?;
    let account = AppleAccount::builder(apple_id)
        .anisette_provider(anisette(AccountStorage::new(SERVICE, apple_id))?)
        .build()
        .await
        .map_err(|e| LinkError::Apple("phone.apple_network", short(&e)))?;
    Ok(DeveloperSession::new(
        token,
        adsid,
        account.grandslam_client.clone(),
        account.anisette_generator.clone(),
    ))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), &target)?;
        }
    }
    Ok(())
}

/// Signs and installs the app (`.app` folder or `.ipa`) on the phone over the cable, with the
/// stored login. Installing over the same app keeps its files — this is also how the hub renews
/// until profile-only renewal (П3) is proven.
pub async fn install(
    apple_id: &str,
    udid: &str,
    app: &Path,
) -> Result<std::time::Duration, LinkError> {
    if !app.exists() {
        return Err(LinkError::Apple("phone.app_missing", String::new()));
    }
    let dev = session(apple_id).await?;
    let mut mux = UsbmuxdConnection::default()
        .await
        .map_err(|e| LinkError::AppleServiceMissing(format!("{e:?}")))?;
    let device = mux
        .get_devices()
        .await
        .map_err(|e| LinkError::Device(format!("{e:?}")))?
        .into_iter()
        .find(|d| d.udid == udid)
        .ok_or(LinkError::Device("not connected".into()))?;
    let provider = device.to_provider(UsbmuxdAddr::default(), "sirtone");

    // isideload signs a `.app` in place and deletes it after installing: hand it a copy.
    let work = std::env::temp_dir().join(format!("sirtone-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let input: PathBuf = if app.is_dir() {
        let name = app
            .file_name()
            .ok_or(LinkError::Apple("phone.app_missing", String::new()))?;
        let dst = work.join(name);
        copy_dir(app, &dst).map_err(|e| LinkError::Apple("phone.install", e.to_string()))?;
        dst
    } else {
        std::fs::create_dir_all(&work)
            .map_err(|e| LinkError::Apple("phone.install", e.to_string()))?;
        let dst = work.join(app.file_name().unwrap_or_default());
        std::fs::copy(app, &dst).map_err(|e| LinkError::Apple("phone.install", e.to_string()))?;
        dst
    };

    let mut sideloader =
        SideloaderBuilder::<MaxCertsCallbackBox, ExtensionsBehaviorCallbackBox>::new(
            dev,
            apple_id.to_string(),
        )
        .team_selection(TeamSelection::First)
        // Never revoke: a revoked certificate stops every app signed with it.
        .max_certs_behavior(MaxCertsBehavior::Error)
        .storage(Box::new(AccountStorage::new(SERVICE, apple_id)))
        .machine_name(MACHINE.to_string())
        .build();
    let started = std::time::Instant::now();
    let r = sideloader
        .install_app(
            &provider,
            input,
            false,
            None::<fn(f32) -> std::future::Ready<()>>,
            None,
        )
        .await;
    let _ = std::fs::remove_dir_all(&work);
    r.map_err(|e| LinkError::Apple("phone.install", short(&e)))?;
    Ok(started.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_keys_are_separate_and_stable() {
        let a = AccountStorage::new(SERVICE, "Someone@Example.com ");
        let b = AccountStorage::new(SERVICE, "someone@example.com");
        let c = AccountStorage::new(SERVICE, "other@example.com");
        assert_eq!(a.k("token"), b.k("token"));
        assert_ne!(a.k("token"), c.k("token"));
        assert!(!a.k("token").contains("example"), "no e-mail in key names");
    }

    /// D-132: what the hub shows or logs never carries what a report's attachments hold.
    #[test]
    fn errors_keep_the_first_line_only() {
        let report: rootcause::Report =
            rootcause::report!("login failed").attach("password=hunter2 GsIdmsToken=SECRET");
        let e = LinkError::Apple("phone.apple_login", short(&report));
        let shown = format!("{e} {e:?}");
        assert!(shown.contains("login failed"));
        assert!(
            !shown.contains("hunter2") && !shown.contains("SECRET"),
            "{shown}"
        );
        assert_eq!(e.code(), "phone.apple_login");
    }
}
