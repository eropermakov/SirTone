//! Hub «Телефоны» (stage 8, FR-HUB-001…004, D-132, D-139, D-143): what phones the PC sees, and
//! the iPhone wizard — Apple ID login (password and 2FA through the form, never stored or logged),
//! then signing and installing over the cable. Transfer (M6) and Wi-Fi (П4б) come later.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_devicelink::apple::{self, TwoFactorAnswer, TwoFactorAsk};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

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

// ---------------------------------------------------------------------------------------------
// The iPhone wizard (MR §6.1 steps 6–7).

/// How long the login waits for the person's 2FA answer before giving up.
const TWO_FACTOR_WAIT: Duration = Duration::from_secs(300);
/// Dev source of the app until GitHub Releases ships the unsigned IPA (`docs/11_DEV_ENV.md` §5).
const PACKAGE_ENV: &str = "SIRTONE_IOS_APP";

/// What the hub remembers per phone (not secret: the Apple ID's tokens live in Credential
/// Manager, D-132). `phones.json` in the app's data folder.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PhoneRecord {
    apple_id: String,
    installed_at_ms: Option<u64>,
}

pub struct PhoneHub {
    file: PathBuf,
    records: Mutex<BTreeMap<String, PhoneRecord>>,
    /// The waiting login's 2FA answers (one login at a time).
    answers: Mutex<Option<tokio::sync::mpsc::UnboundedSender<TwoFactorAnswer>>>,
    login: tokio::sync::Mutex<()>,
    installing: Mutex<HashSet<String>>,
}

impl PhoneHub {
    pub fn open(data_dir: &std::path::Path) -> Self {
        let file = data_dir.join("phones.json");
        let records = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            file,
            records: Mutex::new(records),
            answers: Mutex::new(None),
            login: tokio::sync::Mutex::new(()),
            installing: Mutex::new(HashSet::new()),
        }
    }

    fn record(&self, udid: &str) -> Option<PhoneRecord> {
        self.records.lock().ok()?.get(udid).cloned()
    }

    fn save(&self, all: &BTreeMap<String, PhoneRecord>) {
        if let Ok(bytes) = serde_json::to_vec_pretty(all) {
            let tmp = self.file.with_extension("json.part");
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, &self.file);
            }
        }
    }

    fn remember(&self, udid: &str, r: PhoneRecord) {
        let Ok(mut all) = self.records.lock() else {
            return;
        };
        all.insert(udid.to_string(), r);
        self.save(&all);
    }

    fn forget(&self, udid: &str) -> Option<PhoneRecord> {
        let mut all = self.records.lock().ok()?;
        let r = all.remove(udid);
        self.save(&all);
        r
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// `LinkError` code → `AppError` (the UI words it; details are the code only, never secrets).
fn app_error(e: &sirtone_devicelink::LinkError) -> AppError {
    let code = match e.code() {
        "phone.apple_signed_out" => ErrorCode::AppleSignedOut,
        "phone.apple_login" => ErrorCode::AppleLogin,
        "phone.apple_network" | "phone.apple_anisette" => {
            return AppError::new(ErrorCode::RemoteError)
                .retryable()
                .with_details(e.code());
        }
        "phone.app_missing" => ErrorCode::PhonePackageMissing,
        "phone.install" => ErrorCode::PhoneInstall,
        "phone.apple_service_missing" | "phone.device" => ErrorCode::DeviceUnavailable,
        _ => ErrorCode::Internal,
    };
    AppError::new(code).with_details(e.code())
}

/// The phone's Apple ID as the wizard needs it: skip the login step when it is still valid.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PhoneAccount {
    pub apple_id: Option<String>,
    pub signed_in: bool,
    /// Unix ms when the Apple login ends (then the person signs in again).
    pub login_until_ms: Option<f64>,
    pub installed_at_ms: Option<f64>,
    /// The app package is available to install.
    pub package: bool,
}

fn package() -> Option<PathBuf> {
    std::env::var_os(PACKAGE_ENV)
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

#[tauri::command(async)]
#[specta::specta]
pub fn phone_account(hub: State<'_, PhoneHub>, udid: String) -> PhoneAccount {
    let r = hub.record(&udid);
    let s = r.as_ref().and_then(|r| apple::signed_in(&r.apple_id));
    PhoneAccount {
        apple_id: r.as_ref().map(|r| r.apple_id.clone()),
        signed_in: s.is_some_and(|s| s.token_expiry_ms > now_ms()),
        login_until_ms: s.map(|s| s.token_expiry_ms as f64),
        installed_at_ms: r.and_then(|r| r.installed_at_ms).map(|t| t as f64),
        package: package().is_some(),
    }
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TwoFactorRequest {
    /// The code went by SMS (else to the person's Apple devices).
    pub sms: bool,
    /// The previous code did not work.
    pub retry: bool,
}

#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind", content = "code")]
pub enum TwoFactorReply {
    Code(String),
    SendToDevices,
    Resend,
    Abort,
}

/// Logs in with the Apple ID of this phone's owner (wizard step 6). Each 2FA request goes to
/// `asks`; the form answers with [`phone_apple_2fa`]. The password is dropped when this returns.
#[tauri::command(async)]
#[specta::specta]
pub async fn phone_apple_login(
    app: AppHandle,
    hub: State<'_, PhoneHub>,
    udid: String,
    apple_id: String,
    password: String,
    asks: Channel<TwoFactorRequest>,
) -> Result<(), AppError> {
    if crate::net::offline(&app) {
        return Err(AppError::new(ErrorCode::NetworkOffline));
    }
    let apple_id = apple_id.trim().to_string();
    if apple_id.is_empty() || password.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    let Ok(_one) = hub.login.try_lock() else {
        return Err(AppError::new(ErrorCode::DeviceBusy));
    };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    if let Ok(mut a) = hub.answers.lock() {
        *a = Some(tx);
    }
    let rx = tokio::sync::Mutex::new(rx);
    let ask = |q: TwoFactorAsk| {
        let _ = asks.send(TwoFactorRequest {
            sms: q.sms,
            retry: q.retry,
        });
        let rx = &rx;
        async move {
            let mut rx = rx.lock().await;
            match tokio::time::timeout(TWO_FACTOR_WAIT, rx.recv()).await {
                Ok(Some(a)) => a,
                _ => TwoFactorAnswer::Abort,
            }
        }
    };
    let r = apple::login(&apple_id, &password, ask).await;
    drop(password);
    if let Ok(mut a) = hub.answers.lock() {
        *a = None;
    }
    r.map_err(|e| app_error(&e))?;
    let installed_at_ms = hub.record(&udid).and_then(|r| r.installed_at_ms);
    hub.remember(
        &udid,
        PhoneRecord {
            apple_id,
            installed_at_ms,
        },
    );
    Ok(())
}

/// The person's answer to the waiting 2FA request.
#[tauri::command(async)]
#[specta::specta]
pub fn phone_apple_2fa(hub: State<'_, PhoneHub>, reply: TwoFactorReply) {
    let a = match reply {
        TwoFactorReply::Code(c) => TwoFactorAnswer::Code(c.trim().to_string()),
        TwoFactorReply::SendToDevices => TwoFactorAnswer::SendToDevices,
        TwoFactorReply::Resend => TwoFactorAnswer::Resend,
        TwoFactorReply::Abort => TwoFactorAnswer::Abort,
    };
    if let Ok(g) = hub.answers.lock()
        && let Some(tx) = g.as_ref()
    {
        let _ = tx.send(a);
    }
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    /// 0…1.
    pub fraction: f32,
}

/// Signs and installs Сиртон on the phone over the cable (wizard step 7); over an existing
/// install it keeps the app's files (that is also how the hub renews until П3).
#[tauri::command(async)]
#[specta::specta]
pub async fn phone_install(
    app: AppHandle,
    hub: State<'_, PhoneHub>,
    udid: String,
    progress: Channel<InstallProgress>,
) -> Result<(), AppError> {
    if crate::net::offline(&app) {
        return Err(AppError::new(ErrorCode::NetworkOffline));
    }
    let Some(record) = hub.record(&udid) else {
        return Err(AppError::new(ErrorCode::AppleSignedOut));
    };
    let Some(pkg) = package() else {
        return Err(AppError::new(ErrorCode::PhonePackageMissing));
    };
    if !hub
        .installing
        .lock()
        .is_ok_and(|mut s| s.insert(udid.clone()))
    {
        return Err(AppError::new(ErrorCode::DeviceBusy));
    }
    let report = |f: f32| {
        let _ = progress.send(InstallProgress { fraction: f });
    };
    report(0.0);
    let r = apple::install(&record.apple_id, &udid, &pkg, &report).await;
    if let Ok(mut s) = hub.installing.lock() {
        s.remove(&udid);
    }
    let took = r.map_err(|e| app_error(&e))?;
    tracing::info!(target: "sirtone::phones", secs = took.as_secs_f32(), "installed");
    hub.remember(
        &udid,
        PhoneRecord {
            apple_id: record.apple_id,
            installed_at_ms: Some(now_ms()),
        },
    );
    Ok(())
}

/// «Отключить телефон»: forgets the phone and, if no other phone uses it, its Apple ID (D-132).
#[tauri::command(async)]
#[specta::specta]
pub fn phone_forget(hub: State<'_, PhoneHub>, udid: String) -> Result<(), AppError> {
    let Some(r) = hub.forget(&udid) else {
        return Ok(());
    };
    let shared = hub
        .records
        .lock()
        .is_ok_and(|all| all.values().any(|o| o.apple_id == r.apple_id));
    if !shared {
        apple::forget(&r.apple_id).map_err(|e| app_error(&e))?;
    }
    Ok(())
}
