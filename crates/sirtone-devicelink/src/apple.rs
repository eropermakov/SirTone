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

use idevice::usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdConnection};
use isideload::anisette::remote_v3::RemoteV3AnisetteProvider;
use isideload::auth::apple_account::{
    AppToken, AppleAccount, TwoFactorCallbackParams, TwoFactorCallbackResponse,
};
use isideload::dev::developer_session::DeveloperSession;
use isideload::dev::devices::DevicesApi;
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
pub(crate) fn short(e: &rootcause::Report) -> String {
    e.to_string()
        .lines()
        .find_map(|l| l.trim_start().strip_prefix('●'))
        .unwrap_or("")
        .trim()
        .to_string()
}

/// An Apple failure under `code`, except a closed session: Apple ends the developer session
/// (error 1100) long before the stored token's own expiry, and then only a new login helps.
pub(crate) fn apple_fail(code: &'static str, e: &rootcause::Report) -> LinkError {
    let full = e.to_string();
    if full.contains("Developer error 1100") {
        return LinkError::Apple("phone.apple_signed_out", short(e));
    }
    // The limits of a free Apple ID (MR §6.3), as Apple and the phone word them
    // (developer forums threads 701805, 49304; AltStore FAQ «error codes»).
    let lower = full.to_lowercase();
    if lower.contains("maximum app id limit") {
        return LinkError::Apple("phone.app_id_limit", short(e));
    }
    if lower.contains("0xe8008021") || lower.contains("maximum number of apps for free development")
    {
        return LinkError::Apple("phone.app_limit", short(e));
    }
    if full.contains("DeviceLocked") || full.contains("PasswordProtected") {
        return LinkError::Apple("phone.locked", short(e));
    }
    LinkError::Apple(code, short(e))
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

/// Signs and installs the app (`.app` folder or `.ipa`) on the phone, with the stored login:
/// over the cable when the phone is on it, else over Wi-Fi when it has a Wi-Fi record
/// ([`crate::wifi`], check П4б). Installing over the same app keeps its files — this is also how
/// the hub renews until profile-only renewal (П3) is proven.
/// `progress` gets 0…1: signing reaches 0.5, then the transfer and install (over Wi-Fi with
/// steps, over the cable 0.6 until done — isideload reports no finer steps there), 1.0 when done.
pub async fn install(
    apple_id: &str,
    udid: &str,
    app: &Path,
    progress: &(dyn Fn(f32) + Send + Sync),
) -> Result<std::time::Duration, LinkError> {
    if !app.exists() {
        return Err(LinkError::Apple("phone.app_missing", String::new()));
    }
    let started = std::time::Instant::now();
    // The cable first; without the Apple service or the cable — Wi-Fi, if set up.
    let cable = match UsbmuxdConnection::default().await {
        Ok(mut mux) => mux
            .get_devices()
            .await
            .map_err(|e| LinkError::Device(format!("{e:?}")))?
            .into_iter()
            .find(|d| d.udid == udid && matches!(d.connection_type, Connection::Usb)),
        Err(e) if !crate::wifi::is_paired(udid) => {
            return Err(LinkError::AppleServiceMissing(format!("{e:?}")));
        }
        Err(_) => None,
    };
    if cable.is_none() && !crate::wifi::is_paired(udid) {
        return Err(LinkError::Device("not connected".into()));
    }
    let dev = session(apple_id).await?;

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
    let signing = |p: f32| {
        // Signing ends at 0.5; the transfer and install follow.
        progress(p.min(0.5));
        std::future::ready(())
    };
    let r = match cable {
        Some(device) => {
            let provider = device.to_provider(UsbmuxdAddr::default(), "sirtone");
            sideloader
                .install_app(
                    &provider,
                    input,
                    false,
                    Some(|p: f32| {
                        progress(if p >= 0.5 { 0.6 } else { p });
                        std::future::ready(())
                    }),
                    None,
                )
                .await
                .map(|_| ())
                .map_err(|e| apple_fail("phone.install", &e))
        }
        None => {
            Box::pin(install_wifi(
                &mut sideloader,
                udid,
                input,
                &signing,
                progress,
            ))
            .await
        }
    };
    let _ = std::fs::remove_dir_all(&work);
    r?;
    progress(1.0);
    Ok(started.elapsed())
}

/// The Wi-Fi leg: what `Sideloader::install_app` does over the cable — register the phone, sign —
/// then AFC + installation_proxy through the RemotePairing tunnel (`install_app_rsd`).
async fn install_wifi<F, Fut>(
    sideloader: &mut isideload::sideload::sideloader::Sideloader<
        MaxCertsCallbackBox,
        ExtensionsBehaviorCallbackBox,
    >,
    udid: &str,
    input: PathBuf,
    signing: &F,
    progress: &(dyn Fn(f32) + Send + Sync),
) -> Result<(), LinkError>
where
    F: Fn(f32) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut tunnel = Box::pin(crate::wifi::open(udid, std::time::Duration::from_secs(10))).await?;
    let fail = |e: rootcause::Report| apple_fail("phone.install", &e);
    let team = sideloader.get_team().await.map_err(fail)?;
    let name = tunnel.name.clone().unwrap_or_else(|| "iPhone".into());
    sideloader
        .get_dev_session()
        .ensure_device_registered(&team, &name, udid, None)
        .await
        .map_err(fail)?;
    let (signed, _) = sideloader
        .sign_app(input, Some(team), false, Some(signing), None, Some(udid))
        .await
        .map_err(fail)?;
    let r = isideload::sideload::install::install_app_rsd(
        &mut tunnel.handle,
        &mut tunnel.rsd,
        &signed,
        |pct| progress(0.5 + 0.5 * (pct.min(100) as f32) / 100.0),
    )
    .await
    .map_err(fail);
    // Only the signed bundle itself (as isideload does): for an `.ipa` it sits in the shared
    // temp folder, so never its parent.
    let _ = std::fs::remove_dir_all(&signed);
    r
}

/// The app's bundle identifier before the team suffix (D-136).
const BUNDLE: &str = "studio.serkov.sirtone.mobile";

/// `ExpirationDate` of a provisioning profile (CMS-wrapped XML plist), as written there (ISO 8601).
fn profile_expiry(profile: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(profile);
    let after = &text[text.find("<key>ExpirationDate</key>")?..];
    let start = after.find("<date>")? + "<date>".len();
    let end = after[start..].find("</date>")?;
    Some(after[start..start + end].trim().to_string())
}

/// Unix ms of an ISO 8601 UTC time as profiles write it (`2026-11-14T10:00:00Z`).
pub fn iso_to_ms(s: &str) -> Option<u64> {
    let s = s.trim().trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time
        .split(':')
        .map(|x| x.split('.').next().unwrap_or("0").parse::<i64>());
    let (hh, mm, ss) = (
        t.next()?.ok()?,
        t.next()?.ok()?,
        t.next().unwrap_or(Ok(0)).ok()?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from the civil date (H. Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(((days * 24 + hh) * 60 + mm) * 60 + ss)
        .ok()
        .map(|s| s * 1000)
}

/// Renews the app's signature without reinstalling (check П3): a fresh provisioning profile for
/// the app's App ID from Apple, installed by `misagent` — over the cable when the phone is on it,
/// else through the Wi-Fi tunnel. The certificate stays the same, so the app keeps running
/// (AltStore `RefreshAppOperation`, `01d` [M11]). Returns the new profile's end (ISO 8601).
/// `phone.renew_no_app_id` — Apple no longer has the App ID (expired): reinstall instead.
pub async fn renew(apple_id: &str, udid: &str) -> Result<String, LinkError> {
    use idevice::misagent::MisagentClient;
    use idevice::{IdeviceService, RsdService};
    use isideload::dev::app_ids::AppIdsApi;

    let cable = match UsbmuxdConnection::default().await {
        Ok(mut mux) => mux.get_devices().await.ok().and_then(|ds| {
            ds.into_iter()
                .find(|d| d.udid == udid && matches!(d.connection_type, Connection::Usb))
        }),
        Err(_) => None,
    };
    if cable.is_none() && !crate::wifi::is_paired(udid) {
        return Err(LinkError::Device("not connected".into()));
    }
    let dev = session(apple_id).await?;
    let mut sideloader =
        SideloaderBuilder::<MaxCertsCallbackBox, ExtensionsBehaviorCallbackBox>::new(
            dev,
            apple_id.to_string(),
        )
        .team_selection(TeamSelection::First)
        .max_certs_behavior(MaxCertsBehavior::Error)
        .storage(Box::new(AccountStorage::new(SERVICE, apple_id)))
        .machine_name(MACHINE.to_string())
        .build();
    let fail = |code: &'static str| move |e: rootcause::Report| apple_fail(code, &e);
    let team = sideloader.get_team().await.map_err(fail("phone.renew"))?;
    let wanted = format!("{BUNDLE}.{}", team.team_id);
    let dev = sideloader.get_dev_session();
    let ids = dev
        .list_app_ids(&team, None)
        .await
        .map_err(fail("phone.renew"))?;
    let app_id = ids
        .app_ids
        .into_iter()
        .find(|a| a.identifier == wanted)
        .ok_or(LinkError::Apple("phone.renew_no_app_id", String::new()))?;
    let profile = dev
        .download_team_provisioning_profile(&team, &app_id, None)
        .await
        .map_err(fail("phone.renew"))?;
    let bytes: Vec<u8> = profile.encoded_profile.as_ref().to_vec();
    let expiry = profile_expiry(&bytes).unwrap_or_default();
    let mis = |e: idevice::IdeviceError| LinkError::Apple("phone.renew", format!("{e:?}"));
    match cable {
        Some(device) => {
            let provider = device.to_provider(UsbmuxdAddr::default(), "sirtone");
            let mut c = MisagentClient::connect(&provider).await.map_err(mis)?;
            c.install(bytes).await.map_err(mis)?;
        }
        None => {
            let mut t =
                Box::pin(crate::wifi::open(udid, std::time::Duration::from_secs(10))).await?;
            let mut c = MisagentClient::connect_rsd(&mut t.handle, &mut t.rsd)
                .await
                .map_err(mis)?;
            c.install(bytes).await.map_err(mis)?;
        }
    }
    Ok(expiry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_dates_become_unix_ms() {
        assert_eq!(iso_to_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_ms("2026-11-14T10:00:00Z"), Some(1_794_650_400_000));
        assert_eq!(iso_to_ms("2024-02-29T23:59:59Z"), Some(1_709_251_199_000));
        assert_eq!(iso_to_ms("not a date"), None);
    }

    #[test]
    fn reads_the_profile_end() {
        let mut p = vec![0x30, 0x82];
        p.extend_from_slice(
            b"<plist><dict><key>CreationDate</key><date>2026-11-07T10:00:00Z</date>",
        );
        p.extend_from_slice(
            b"<key>ExpirationDate</key>
	<date>2026-11-14T10:00:00Z</date></dict>",
        );
        p.extend_from_slice(b"</plist>junk");
        assert_eq!(profile_expiry(&p).as_deref(), Some("2026-11-14T10:00:00Z"));
        assert_eq!(profile_expiry(b"no plist"), None);
    }

    #[test]
    fn account_keys_are_separate_and_stable() {
        let a = AccountStorage::new(SERVICE, "Someone@Example.com ");
        let b = AccountStorage::new(SERVICE, "someone@example.com");
        let c = AccountStorage::new(SERVICE, "other@example.com");
        assert_eq!(a.k("token"), b.k("token"));
        assert_ne!(a.k("token"), c.k("token"));
        assert!(!a.k("token").contains("example"), "no e-mail in key names");
    }

    #[test]
    fn closed_session_asks_for_login() {
        let r: rootcause::Report = rootcause::report!(isideload::SideloadError::DeveloperError(
            1100,
            "Your session has expired. Please log in.".into()
        ))
        .context("Failed to list developer teams")
        .into_dynamic();
        assert_eq!(
            apple_fail("phone.install", &r).code(),
            "phone.apple_signed_out"
        );
        let limit: rootcause::Report = rootcause::report!(isideload::SideloadError::DeveloperError(
            9401,
            "Your maximum App ID limit has been reached. You may create up to 10 App IDs every 7 days."
                .into()
        ))
        .into_dynamic();
        assert_eq!(
            apple_fail("phone.install", &limit).code(),
            "phone.app_id_limit"
        );
        let three: rootcause::Report = rootcause::report!(
            "ApplicationVerificationFailed: The maximum number of apps for free development profiles has been reached. (0xE8008021)"
        );
        assert_eq!(
            apple_fail("phone.install", &three).code(),
            "phone.app_limit"
        );
        let other: rootcause::Report = rootcause::report!("boom");
        assert_eq!(apple_fail("phone.install", &other).code(), "phone.install");
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
