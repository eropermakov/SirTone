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
/// A phone seen only over Wi-Fi carries no name or model (they need the tunnel): the ones it
/// had on its last cable connection are shown instead.
#[tauri::command(async)]
#[specta::specta]
pub async fn phones_list(app: AppHandle) -> PhonesStatus {
    use sirtone_devicelink as dl;
    use tauri::Manager as _;
    let hub = app.state::<PhoneHub>();
    let (mut phones, errors) = dl::all_phones().await;
    for p in &mut phones {
        if p.platform != dl::Platform::Ios {
            continue;
        }
        if p.link == dl::Link::Usb && p.name.is_some() {
            hub.remember_looks(&p.id, p.name.clone(), p.model.clone(), p.os_version.clone());
        } else if p.name.is_none()
            && let Some(r) = hub.record(&p.id)
        {
            p.name = r.name;
            p.model = p.model.take().or(r.model);
            p.os_version = p.os_version.take().or(r.os_version);
        }
    }
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
/// A local package instead of the channel's (development, `docs/11_DEV_ENV.md` §5).
const PACKAGE_ENV: &str = "SIRTONE_IOS_APP";

/// What the hub remembers per phone (not secret: the Apple ID's tokens live in Credential
/// Manager, D-132). `phones.json` in the app's data folder.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PhoneRecord {
    apple_id: String,
    installed_at_ms: Option<u64>,
    /// As on the last cable connection (shown when the phone is on Wi-Fi only).
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    os_version: Option<String>,
    /// When the app's signature ends (unix ms): a fresh install gives 7 days, a renewal the new
    /// profile's end (FR-HUB-RENEW).
    #[serde(default)]
    works_until_ms: Option<u64>,
    /// The last renewal that failed (unix ms): the planner waits before asking Apple again.
    #[serde(default)]
    renew_failed_ms: Option<u64>,
}

impl PhoneRecord {
    /// The signature's end: as known, else 7 days from the install.
    fn works_until(&self) -> Option<u64> {
        self.works_until_ms
            .or_else(|| self.installed_at_ms.map(|t| t + WEEK_MS))
    }
}

/// A free Apple ID signs for 7 days (MR §1).
const WEEK_MS: u64 = 7 * 24 * 3_600_000;
/// Renew this long before the end (MR §10: a 3-day window, a try whenever the phone shows up).
const RENEW_WINDOW_MS: u64 = 3 * 24 * 3_600_000;
/// How often the tray-resident hub looks.
const RENEW_EVERY: Duration = Duration::from_secs(600);
/// After a failed renewal: don't call Apple's developer services again before this.
const RENEW_BACKOFF_MS: u64 = 6 * 3_600_000;

/// Phones whose signature is inside the renewal window now.
fn due(records: &BTreeMap<String, PhoneRecord>, now: u64) -> Vec<(String, String)> {
    records
        .iter()
        .filter(|(_, r)| r.works_until().is_some_and(|u| now + RENEW_WINDOW_MS >= u))
        .filter(|(_, r)| {
            r.renew_failed_ms
                .is_none_or(|f| now >= f + RENEW_BACKOFF_MS)
        })
        .map(|(udid, r)| (udid.clone(), r.apple_id.clone()))
        .collect()
}

/// The renewal planner (M2.3, FR-HUB-RENEW): every 10 minutes, phones in the 3-day window that
/// are reachable (cable or Wi-Fi) get a fresh profile — no reinstall, the music keeps playing
/// (check П3). Offline, or while a phone is being installed or filled, it waits.
pub fn start_renewals(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager as _;
        loop {
            tokio::time::sleep(RENEW_EVERY).await;
            if crate::net::offline(&app) {
                continue;
            }
            let hub = app.state::<PhoneHub>();
            let list = hub
                .records
                .lock()
                .map(|r| due(&r, now_ms()))
                .unwrap_or_default();
            if list.is_empty() {
                continue;
            }
            let (seen, _) = sirtone_devicelink::all_phones().await;
            for (udid, apple_id) in list {
                if !seen.iter().any(|p| p.id == udid) || apple::signed_in(&apple_id).is_none() {
                    continue;
                }
                if !hub
                    .installing
                    .lock()
                    .is_ok_and(|mut s| s.insert(udid.clone()))
                {
                    continue;
                }
                let r = apple::renew(&apple_id, &udid).await;
                if let Ok(mut s) = hub.installing.lock() {
                    s.remove(&udid);
                }
                match r {
                    Ok(until) => {
                        let ms = apple::iso_to_ms(&until).unwrap_or(now_ms() + WEEK_MS);
                        if let Some(rec) = hub.record(&udid) {
                            hub.remember(
                                &udid,
                                PhoneRecord {
                                    works_until_ms: Some(ms),
                                    renew_failed_ms: None,
                                    ..rec
                                },
                            );
                        }
                        tracing::info!(target: "sirtone::phones", "renewed");
                    }
                    Err(e) => {
                        if let Some(rec) = hub.record(&udid) {
                            hub.remember(
                                &udid,
                                PhoneRecord {
                                    renew_failed_ms: Some(now_ms()),
                                    ..rec
                                },
                            );
                        }
                        tracing::info!(target: "sirtone::phones", code = e.code(), "renew");
                    }
                }
            }
        }
    });
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

    /// Keeps a known phone's name, model and iOS (only for phones the hub installed on).
    fn remember_looks(
        &self,
        udid: &str,
        name: Option<String>,
        model: Option<String>,
        os_version: Option<String>,
    ) {
        let Ok(mut all) = self.records.lock() else {
            return;
        };
        let Some(r) = all.get_mut(udid) else {
            return;
        };
        if r.name == name && r.model == model && r.os_version == os_version {
            return;
        }
        (r.name, r.model, r.os_version) = (name, model, os_version);
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
    // A locked phone answers the cable with «DeviceLocked» / «PasswordProtected».
    if let sirtone_devicelink::LinkError::Device(m) = e
        && (m.contains("DeviceLocked") || m.contains("PasswordProtected"))
    {
        return AppError::new(ErrorCode::PhoneLocked).retryable();
    }
    let code = match e.code() {
        "phone.locked" => ErrorCode::PhoneLocked,
        "phone.app_limit" => ErrorCode::PhoneAppLimit,
        "phone.app_id_limit" => ErrorCode::PhoneAppIdLimit,
        "phone.apple_signed_out" => ErrorCode::AppleSignedOut,
        "phone.apple_login" => ErrorCode::AppleLogin,
        "phone.apple_network" | "phone.apple_anisette" => {
            return AppError::new(ErrorCode::RemoteError)
                .retryable()
                .with_details(e.code());
        }
        "phone.app_missing" => ErrorCode::PhonePackageMissing,
        "phone.install" | "phone.wifi_tunnel" | "phone.wifi_pair" | "phone.wifi_not_paired" => {
            ErrorCode::PhoneInstall
        }
        "phone.transfer" | "phone.app_not_installed" => ErrorCode::PhoneTransfer,
        // Not on the cable and not seen on Wi-Fi (screen off, another network): «телефон не виден».
        "phone.apple_service_missing"
        | "phone.device"
        | "phone.wifi_not_found"
        | "phone.wifi_unreachable" => ErrorCode::DeviceUnavailable,
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
    /// Unix ms when the app on the phone stops opening unless renewed (S11, «Работает ещё N дней»).
    pub works_until_ms: Option<f64>,
    /// The app package is available to install.
    pub package: bool,
}

fn dev_package() -> Option<PathBuf> {
    std::env::var_os(PACKAGE_ENV)
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

#[tauri::command(async)]
#[specta::specta]
pub fn phone_account(app: AppHandle, hub: State<'_, PhoneHub>, udid: String) -> PhoneAccount {
    let r = hub.record(&udid);
    let s = r.as_ref().and_then(|r| apple::signed_in(&r.apple_id));
    PhoneAccount {
        apple_id: r.as_ref().map(|r| r.apple_id.clone()),
        signed_in: s.is_some_and(|s| s.token_expiry_ms > now_ms()),
        login_until_ms: s.map(|s| s.token_expiry_ms as f64),
        installed_at_ms: r.as_ref().and_then(|r| r.installed_at_ms).map(|t| t as f64),
        works_until_ms: r.and_then(|r| r.works_until()).map(|t| t as f64),
        // Downloaded on install (D-154): reachable unless the PC is offline.
        package: dev_package().is_some() || !crate::net::offline(&app),
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
    let before = hub.record(&udid).unwrap_or_default();
    hub.remember(&udid, PhoneRecord { apple_id, ..before });
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

/// The app to install: a local development package, else the update channel's (D-154).
async fn ios_package(app: &AppHandle) -> Result<PathBuf, AppError> {
    use tauri::Manager as _;
    if let Some(p) = dev_package() {
        return Ok(p);
    }
    let internal = || AppError::new(ErrorCode::Internal);
    let net = app.state::<crate::net::NetState>().0.clone();
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|_| internal())?
        .join("ios-package");
    // The key of the PC updates, from the one place it is written.
    let pubkey = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .ok_or_else(internal)?
        .to_owned();
    tokio::task::spawn_blocking(move || crate::ios_package::fetch(&net, &dir, &pubkey))
        .await
        .map_err(|_| internal())?
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
    let r = match ios_package(&app).await {
        Ok(pkg) => apple::install(&record.apple_id, &udid, &pkg, &report)
            .await
            .map_err(|e| app_error(&e)),
        Err(e) => Err(e),
    };
    if let Ok(mut s) = hub.installing.lock() {
        s.remove(&udid);
    }
    let took = r?;
    tracing::info!(target: "sirtone::phones", secs = took.as_secs_f32(), "installed");
    // Wizard step 10: Wi-Fi access while the phone is still on the trusted cable (П4б). Best
    // effort and detached — the install already succeeded and the wizard moves on at once; a
    // phone on Wi-Fi only is simply skipped.
    if !sirtone_devicelink::wifi::is_paired(&udid) {
        let id = udid.clone();
        tauri::async_runtime::spawn(async move {
            let pair = sirtone_devicelink::wifi::pair_over_cable(&id);
            match tokio::time::timeout(std::time::Duration::from_secs(15), pair).await {
                Ok(Ok(())) => tracing::info!(target: "sirtone::phones", "wifi paired"),
                Ok(Err(e)) => {
                    tracing::info!(target: "sirtone::phones", code = e.code(), "wifi pair");
                }
                Err(_) => tracing::info!(target: "sirtone::phones", "wifi pair timeout"),
            }
        });
    }
    hub.remember(
        &udid,
        PhoneRecord {
            installed_at_ms: Some(now_ms()),
            works_until_ms: Some(now_ms() + WEEK_MS),
            ..record
        },
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    /// 0…1 of the bytes to send.
    pub fraction: f32,
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TransferDone {
    /// Songs whose music went (new to the phone, D-152).
    pub tracks: u32,
    /// Library files that could not be read (moved since the last scan); the rest went.
    pub missing: u32,
    /// The phone had everything chosen: nothing was sent («На телефоне уже всё есть»).
    pub already: bool,
}

/// «Выбранная» музыка (D-152): groups of the PC library; `None` in `phone_send` — all of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PhoneSelection {
    pub artists: Vec<u32>,
    pub albums: Vec<u32>,
    /// Genre keys, as the genre pages address them.
    pub genres: Vec<String>,
    pub playlists: Vec<u32>,
    pub tracks: Vec<u32>,
}

/// One row of the «Выбранная» picker.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PickItem {
    pub id: u32,
    /// Genres only: the key to choose them by.
    pub key: Option<String>,
    pub name: String,
    /// Albums: the artist.
    pub detail: Option<String>,
    pub tracks: u32,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PickLists {
    pub artists: Vec<PickItem>,
    pub albums: Vec<PickItem>,
    pub genres: Vec<PickItem>,
    pub playlists: Vec<PickItem>,
}

/// The chosen music before the phone is asked what it has (the size shown up front).
#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SelectionSize {
    pub tracks: u32,
    pub bytes: f64,
    pub duration_sec: f64,
}

/// The picker's lists: artists, albums, genres and playlists of the PC library (D-152).
#[tauri::command(async)]
#[specta::specta]
pub fn phone_pick_lists(lib: State<'_, crate::library::Library>) -> Result<PickLists, AppError> {
    use sirtone_library::collection;
    const ALL: u32 = 1_000_000;
    lib.with_read(|c| {
        let item = |id: i64, name: String, detail: Option<String>, tracks: u32| PickItem {
            id: u32::try_from(id).unwrap_or_default(),
            key: None,
            name,
            detail,
            tracks,
        };
        Ok(PickLists {
            artists: collection::artists_page(c, None, ALL)?
                .0
                .into_iter()
                .map(|a| item(a.id, a.name, None, a.tracks))
                .collect(),
            albums: collection::albums_page(c, None, ALL)?
                .0
                .into_iter()
                .map(|a| item(a.id, a.title, Some(a.artist), a.tracks))
                .collect(),
            genres: collection::genres(c)?
                .into_iter()
                .map(|g| PickItem {
                    id: 0,
                    key: Some(g.key),
                    name: g.name,
                    detail: None,
                    tracks: g.tracks,
                })
                .collect(),
            playlists: sirtone_library::playlists::list(c)?
                .into_iter()
                .map(|p| {
                    let n = sirtone_library::playlists::tracks(c, p.id)?.len() as u32;
                    Ok(item(p.id, p.name, None, n))
                })
                .collect::<Result<_, sirtone_library::LibraryError>>()?,
        })
    })
}

/// Track ids of a selection (each once) and the names of its playlists.
fn chosen_tracks(
    c: &rusqlite::Connection,
    sel: &PhoneSelection,
) -> Result<(Vec<i64>, HashSet<String>), sirtone_library::LibraryError> {
    use sirtone_library::{collection, playlists};
    let mut ids: Vec<i64> = sel.tracks.iter().map(|&t| i64::from(t)).collect();
    for &a in &sel.artists {
        ids.extend(
            collection::artist_tracks(c, i64::from(a))?
                .iter()
                .map(|t| t.id),
        );
    }
    for &a in &sel.albums {
        ids.extend(
            collection::album_tracks(c, i64::from(a))?
                .iter()
                .map(|t| t.id),
        );
    }
    for g in &sel.genres {
        ids.extend(collection::genre_tracks(c, g)?.iter().map(|t| t.id));
    }
    let mut names = HashSet::new();
    for p in playlists::list(c)? {
        if sel.playlists.iter().any(|&id| i64::from(id) == p.id) {
            ids.extend(playlists::tracks(c, p.id)?.iter().map(|t| t.track.id));
            names.insert(p.name);
        }
    }
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(*id));
    Ok((ids, names))
}

/// How much the chosen music is (before the phone says what it has already).
#[tauri::command(async)]
#[specta::specta]
pub fn phone_selection_size(
    lib: State<'_, crate::library::Library>,
    selection: PhoneSelection,
) -> Result<SelectionSize, AppError> {
    lib.with_read(|c| {
        let (ids, _) = chosen_tracks(c, &selection)?;
        let mut st = c.prepare_cached(
            "SELECT size_bytes, duration_ms FROM track WHERE id = ?1 AND state = 'ok'",
        )?;
        let mut s = SelectionSize {
            tracks: 0,
            bytes: 0.0,
            duration_sec: 0.0,
        };
        for id in ids {
            if let Ok((b, d)) =
                st.query_row([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
            {
                s.tracks += 1;
                s.bytes += b as f64;
                s.duration_sec += d as f64 / 1000.0;
            }
        }
        Ok(s)
    })
}

/// What goes to the phone: «Перенести всё» is all three, «Выбрать» the ticked ones (MR §11).
/// Settings and theme do not travel yet.
#[derive(Debug, Clone, Copy, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SendParts {
    pub music: bool,
    pub playlists: bool,
    pub taste: bool,
}

impl SendParts {
    /// The package folder's suffix: another choice is another package (a resume keeps its own).
    fn tag(self) -> String {
        format!(
            "m{}p{}t{}",
            u8::from(self.music),
            u8::from(self.playlists),
            u8::from(self.taste)
        )
    }
}

/// Сиртон is already on this phone (FR-HUB-003): a phone set up from the command line or another
/// PC gets «Перенести на телефон» and «Забрать с телефона» without a reinstall, which would
/// restart its 7-day count. Its renewal date stays unknown until the hub installs or renews it.
#[tauri::command(async)]
#[specta::specta]
pub async fn phone_has_app(udid: String) -> bool {
    sirtone_devicelink::appfiles::has_app(&udid).await
}

/// «Перенести всё» (M6.2, MR §11): the PC library's package — manifest and journal written to
/// the app's cache folder, the music sent straight from the library — into the app's inbox on
/// the phone (cable, else Wi-Fi). The app applies it when opened. A repeat resumes.
#[tauri::command(async)]
#[specta::specta]
pub async fn phone_send(
    app: AppHandle,
    udid: String,
    quality: String,
    parts: SendParts,
    selection: Option<PhoneSelection>,
    progress: Channel<TransferProgress>,
) -> Result<TransferDone, AppError> {
    use tauri::Manager as _;
    let hub = app.state::<PhoneHub>();
    if !hub
        .installing
        .lock()
        .is_ok_and(|mut s| s.insert(udid.clone()))
    {
        return Err(AppError::new(ErrorCode::DeviceBusy));
    }
    let r = send(&app, &udid, &quality, parts, selection.as_ref(), &progress).await;
    if let Ok(mut s) = hub.installing.lock() {
        s.remove(&udid);
    }
    r
}

#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PullDone {
    /// Tracks in the phone's library.
    pub tracks: u32,
    /// Of them, new to this PC (brought over and imported).
    pub new_tracks: u32,
    pub plays: u32,
    pub marks: u32,
    pub playlists: u32,
}

/// «Забрать с телефона» (MR §6.2, FR-HUB-003): the phone's library database is read over the
/// cable or Wi-Fi, its package is written on the PC, only the music this PC does not have is
/// fetched, then the package is applied like any other (new music — ordinary import; plays,
/// marks and playlists once each, D-140). Works with the app closed: nothing runs on the phone.
#[tauri::command(async)]
#[specta::specta]
pub async fn phone_pull(
    app: AppHandle,
    udid: String,
    progress: Channel<TransferProgress>,
) -> Result<PullDone, AppError> {
    use tauri::Manager as _;
    let hub = app.state::<PhoneHub>();
    if !hub
        .installing
        .lock()
        .is_ok_and(|mut s| s.insert(udid.clone()))
    {
        return Err(AppError::new(ErrorCode::DeviceBusy));
    }
    let r = pull(&app, &udid, &progress).await;
    if let Ok(mut s) = hub.installing.lock() {
        s.remove(&udid);
    }
    r
}

/// The phone library's database files: the database and its write-ahead log (the app may be
/// playing and writing; `-shm` is rebuilt by SQLite and never copied).
const PHONE_DB: [&str; 3] = [
    ".sirtone/library.db",
    ".sirtone/library.db-wal",
    ".sirtone/deleted.json",
];

/// Copies the phone library's database into `phone_root/.sirtone`: the two files are read one
/// after the other while the app may write, so it is checked and read once more if it came torn.
/// `false` — no whole copy (no library on the phone yet, or torn twice).
async fn phone_db_snapshot(
    files: &mut sirtone_devicelink::appfiles::AppFiles,
    phone_root: &std::path::Path,
) -> Result<bool, AppError> {
    use sirtone_devicelink::appfiles;
    let db = phone_root.join(".sirtone").join("library.db");
    for _ in 0..2 {
        for name in [
            "library.db",
            "library.db-wal",
            "library.db-shm",
            "deleted.json",
        ] {
            let _ = std::fs::remove_file(db.with_file_name(name));
        }
        let db_files = files
            .sizes(appfiles::LIBRARY, &PHONE_DB.map(String::from))
            .await;
        files
            .pull_files(appfiles::LIBRARY, phone_root, db_files, &|_, _| {})
            .await
            .map_err(|e| app_error(&e))?;
        let whole = sirtone_library::open_existing(&db, sirtone_library::Medium::Fixed)
            .ok()
            .and_then(|c| {
                c.query_row("PRAGMA quick_check", [], |r| r.get::<_, String>(0))
                    .ok()
            })
            .is_some_and(|s| s == "ok");
        if whole {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn pull(
    app: &AppHandle,
    udid: &str,
    progress: &Channel<TransferProgress>,
) -> Result<PullDone, AppError> {
    use sirtone_devicelink::appfiles;
    use tauri::Manager as _;
    let fail = |c: &'static str| AppError::new(ErrorCode::PhoneTransfer).with_details(c);
    let lib = app.state::<crate::library::Library>();
    if lib.root().is_none() {
        return Err(AppError::new(ErrorCode::LibraryRootNotSet));
    }
    let base = app
        .path()
        .app_cache_dir()
        .map_err(|_| AppError::new(ErrorCode::Internal))?
        .join("phone-in")
        .join(appfiles::package_folder(udid));
    let (phone_root, pkg) = (base.join("library"), base.join("package"));
    let db = phone_root.join(".sirtone").join("library.db");
    let _ = progress.send(TransferProgress { fraction: 0.0 });
    let mut files = appfiles::open(udid).await.map_err(|e| app_error(&e))?;

    // 1. A whole snapshot of the database.
    if !phone_db_snapshot(&mut files, &phone_root).await? {
        return Err(fail("transfer.snapshot"));
    }
    let _ = progress.send(TransferProgress { fraction: 0.05 });

    // 2. The phone library's package, written fresh every time (a manifest left from an earlier
    //    pull would miss what was added since); fetched music stays for a resume.
    let _ = std::fs::remove_file(pkg.join("manifest.json"));
    let _ = std::fs::remove_file(pkg.join("events.jsonl"));
    let (db2, root2, pkg2) = (db.clone(), phone_root.clone(), pkg.clone());
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = sirtone_library::open_existing(&db2, sirtone_library::Medium::Fixed)?;
        sirtone_sync::export_with(
            &conn,
            &root2,
            &pkg2,
            sirtone_transfer::Platform::Ios,
            env!("CARGO_PKG_VERSION"),
            sirtone_transfer::Parts {
                music: true,
                playlists: true,
                taste: true,
                settings: false,
            },
            false,
        )
        .map(|_| ())
        .map_err(|e| fail(e.code()))
    })
    .await
    .map_err(|_| AppError::new(ErrorCode::Internal))??;

    // 3. Only the music this PC lacks, by the matcher the apply uses.
    let need: Vec<String> = lib
        .with_read(|c| Ok(sirtone_sync::needed(c, &pkg)))?
        .map_err(|e| fail(e.code()))?
        .into_iter()
        .filter_map(|f| f.strip_prefix("music/").map(str::to_string))
        .collect();
    let music = files.sizes(appfiles::LIBRARY, &need).await;
    let report = |done: u64, total: u64| {
        let f = if total == 0 {
            1.0
        } else {
            done as f32 / total as f32
        };
        let _ = progress.send(TransferProgress {
            fraction: 0.05 + 0.8 * f,
        });
    };
    files
        .pull_files(appfiles::LIBRARY, &pkg.join("music"), music, &report)
        .await
        .map_err(|e| app_error(&e))?;
    drop(files);
    let _ = progress.send(TransferProgress { fraction: 0.85 });

    // 4. Applied on the import worker; the new music shows up in «Импорт» as usual.
    let (app2, pkg2) = (app.clone(), pkg.clone());
    let s = tokio::task::spawn_blocking(move || {
        app2.state::<crate::library::Library>().apply_package(pkg2)
    })
    .await
    .map_err(|_| AppError::new(ErrorCode::Internal))??;
    let _ = std::fs::remove_dir_all(&base);
    let _ = progress.send(TransferProgress { fraction: 1.0 });
    tracing::info!(target: "sirtone::phones", tracks = s.tracks, new = s.new_files, plays = s.plays, "pulled");
    Ok(PullDone {
        tracks: s.tracks,
        new_tracks: s.new_files,
        plays: s.plays,
        marks: s.marks,
        playlists: s.playlists,
    })
}

async fn send(
    app: &AppHandle,
    udid: &str,
    quality: &str,
    chosen: SendParts,
    selection: Option<&PhoneSelection>,
    progress: &Channel<TransferProgress>,
) -> Result<TransferDone, AppError> {
    use sirtone_devicelink::appfiles;
    use tauri::Manager as _;
    let lib = app.state::<crate::library::Library>();
    let root = lib
        .root()
        .ok_or_else(|| AppError::new(ErrorCode::LibraryRootNotSet))?;
    let level = if chosen.music {
        sirtone_audio::encode::Level::parse(quality)
    } else {
        None
    };
    // A chosen playlist travels as a playlist too (D-152).
    let chosen = SendParts {
        playlists: chosen.playlists || selection.is_some_and(|s| !s.playlists.is_empty()),
        ..chosen
    };
    // One package folder per quality, parts and choice: a resume continues the same one, another
    // choice starts clean.
    let cache = app
        .path()
        .app_cache_dir()
        .map_err(|_| AppError::new(ErrorCode::Internal))?;
    let out = cache
        .join("phone-out")
        .join(appfiles::package_folder(udid))
        .join(format!(
            "{}-{}-{:016x}",
            level.map_or("original", |l| l.as_str()),
            chosen.tag(),
            selection.map_or(0, selection_hash)
        ));
    let parts = sirtone_transfer::Parts {
        music: chosen.music,
        playlists: chosen.playlists,
        taste: chosen.taste,
        settings: false,
    };
    let _ = progress.send(TransferProgress { fraction: 0.0 });
    // The phone first: what it has decides what goes (D-152); no point packing for a phone that
    // is gone or full.
    let mut files = appfiles::open(udid).await.map_err(|e| app_error(&e))?;
    let phone_root = cache
        .join("phone-in")
        .join(appfiles::package_folder(udid))
        .join("library");
    let has_db = phone_db_snapshot(&mut files, &phone_root)
        .await
        .unwrap_or(false);
    let _ = progress.send(TransferProgress { fraction: 0.02 });
    let picked = match selection {
        Some(s) => {
            let (ids, names) = lib.with_read(|c| chosen_tracks(c, s))?;
            let uid = lib.with_read(sirtone_library::transfer::library_uid)?;
            let keys: HashSet<String> = ids.iter().map(|id| format!("{uid}:{id}")).collect();
            Some((keys, names))
        }
        None => None,
    };
    let summary = lib.with_write(|c| {
        sirtone_sync::export_with(
            c,
            &root,
            &out,
            sirtone_transfer::Platform::Windows,
            env!("CARGO_PKG_VERSION"),
            parts,
            false,
        )
        .map_err(|e| match e {
            sirtone_sync::SyncError::Library(l) => l,
            other => sirtone_library::LibraryError::Invalid(other.code().into()),
        })
    })?;
    // Only what the phone lacks (D-152): its database decides; without one all is new.
    let (out2, db) = (out.clone(), phone_root.join(".sirtone").join("library.db"));
    let narrowed = tokio::task::spawn_blocking(move || -> Result<_, AppError> {
        let other = if has_db {
            sirtone_library::open_existing(&db, sirtone_library::Medium::Fixed).ok()
        } else {
            None
        };
        let (keys, names) = match &picked {
            Some((k, n)) => (Some(k), Some(n)),
            None => (None, None),
        };
        // What the person deleted on the phone never goes back (D-165).
        let gone = sirtone_sync::read_gone(&db.with_file_name(sirtone_sync::GONE_FILE));
        sirtone_sync::narrow(&out2, other.as_ref(), keys, names, &gone)
            .map_err(|e| AppError::new(ErrorCode::PhoneTransfer).with_details(e.code()))
    })
    .await
    .map_err(|_| AppError::new(ErrorCode::Internal))??;
    if narrowed.is_empty() {
        let _ = std::fs::remove_dir_all(&out);
        let _ = progress.send(TransferProgress { fraction: 1.0 });
        tracing::info!(target: "sirtone::phones", "nothing new for the phone");
        return Ok(TransferDone {
            tracks: 0,
            missing: 0,
            already: true,
        });
    }
    let send_set: HashSet<&str> = narrowed.send.iter().map(String::as_str).collect();
    let manifest = sirtone_transfer::Manifest::read(&out)
        .map_err(|_| AppError::new(ErrorCode::PhoneTransfer).with_details("transfer.manifest"))?;
    let sent = manifest
        .tracks
        .iter()
        .filter(|t| send_set.contains(t.file.as_str()));
    let need = match level {
        // Without music only the manifest and journal go: no music to count.
        _ if !chosen.music => 0,
        // Opus at a fixed rate: duration × bitrate (+ the small manifest and journal).
        Some(l) => sent
            .map(|t| u64::from(t.duration_ms) * u64::from(l.kbps()) / 8)
            .sum::<u64>(),
        None => sent.map(|t| t.size).sum::<u64>(),
    };
    if let Some(free) = files.free_bytes().await {
        let already = files.already_sent(&summary.package_id).await;
        if !appfiles::fits(free, need, already) {
            return Err(AppError::new(ErrorCode::PhoneNoSpace));
        }
    }
    let mut music = if chosen.music {
        let mut m = sirtone_sync::music_sources(&root, &out)
            .map_err(|e| AppError::new(ErrorCode::PhoneTransfer).with_details(e.code()))?;
        m.retain(|(_, file)| send_set.contains(file.as_str()));
        m
    } else {
        Vec::new()
    };
    // «Экономно» / «Максимально» (D-146): the PC makes the Opus copies into the package (faster
    // than the phone, spares its battery); the originals stay untouched.
    if let Some(level) = level {
        let jobs: Vec<Option<sirtone_library::copies::CopyJob>> = music
            .iter()
            .map(|(_, file)| {
                let id = manifest
                    .tracks
                    .iter()
                    .find(|t| &t.file == file)
                    .and_then(|t| t.key.rsplit_once(':'))
                    .and_then(|(_, id)| id.parse::<i64>().ok())?;
                lib.with_read(|c| sirtone_library::copies::job_for(c, &root, id))
                    .ok()
                    .flatten()
            })
            .collect();
        let (out2, prog) = (out.clone(), progress.clone());
        music =
            tokio::task::spawn_blocking(move || encode_package(&out2, music, jobs, level, &prog))
                .await
                .map_err(|_| AppError::new(ErrorCode::Internal))??;
    }
    // With copies the first half of the bar was the encoding.
    let (from, span) = if level.is_some() {
        (0.5, 0.5)
    } else {
        (0.0, 1.0)
    };
    let report = |done: u64, total: u64| {
        let f = if total == 0 {
            1.0
        } else {
            done as f32 / total as f32
        };
        let _ = progress.send(TransferProgress {
            fraction: from + span * f,
        });
    };
    let c = files
        .send_package(&out, &summary.package_id, &music, &report)
        .await
        .map_err(|e| app_error(&e))?;
    let _ = std::fs::remove_dir_all(&out);
    tracing::info!(target: "sirtone::phones", files = c.files, skipped = c.skipped, missing = c.missing, "sent");
    Ok(TransferDone {
        tracks: narrowed.send.len() as u32,
        missing: c.missing,
        already: false,
    })
}

/// A stable name for a choice (the package folder): FNV-1a over its JSON.
fn selection_hash(s: &PhoneSelection) -> u64 {
    serde_json::to_vec(s)
        .unwrap_or_default()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// Makes the Opus copies of `music` inside the package `out` and rewrites its manifest so the
/// phone imports them marked as copies; each copy carries its track's tags and cover (`jobs`, in
/// the order of `music`). Resumes: a finished copy (renamed from `.part`) is kept. Returns what
/// is left to send from the library (originals that could not be read).
fn encode_package(
    out: &std::path::Path,
    music: Vec<(PathBuf, String)>,
    jobs: Vec<Option<sirtone_library::copies::CopyJob>>,
    level: sirtone_audio::encode::Level,
    progress: &Channel<TransferProgress>,
) -> Result<Vec<(PathBuf, String)>, AppError> {
    use sirtone_audio::encode::{self, Tags};
    let fail = |c: &'static str| AppError::new(ErrorCode::PhoneTransfer).with_details(c);
    let mut manifest =
        sirtone_transfer::Manifest::read(out).map_err(|_| fail("transfer.manifest"))?;
    let none = std::sync::atomic::AtomicBool::new(false);
    let total = music.len().max(1);
    let mut left = Vec::new();
    for (i, ((src, file), job)) in music.into_iter().zip(jobs).enumerate() {
        let Some(entry) = manifest.tracks.iter_mut().find(|t| t.file == file) else {
            left.push((src, file));
            continue;
        };
        let id = entry
            .key
            .rsplit_once(':')
            .and_then(|(_, id)| id.parse::<i64>().ok())
            .unwrap_or(i as i64);
        let copy_file = sirtone_library::copies::copy_rel_for(&file, id);
        let dst = copy_file
            .split('/')
            .fold(out.to_path_buf(), |p, s| p.join(s));
        let tags = match &job {
            Some(j) => Tags {
                title: Some(j.title.clone()),
                artist: Some(j.artists.clone()).filter(|a| !a.is_empty()),
                album: j.album.clone(),
                album_artist: j.album_artist.clone(),
                track_no: j.track_no,
                disc_no: j.disc_no,
                year: j.year,
                genre: j.genre.clone(),
                cover: j.cover.as_ref().and_then(|p| std::fs::read(p).ok()),
            },
            None => Tags {
                title: entry.title.clone(),
                artist: entry.artist.clone(),
                album: entry.album.clone(),
                ..Tags::default()
            },
        };
        if dst.exists() || encode::to_opus(&src, &dst, level, &tags, &none).is_ok() {
            entry.file = copy_file;
            entry.size = std::fs::metadata(&dst).map(|m| m.len()).unwrap_or(0);
            // Another file than the original: the phone must not take it for the same bytes.
            entry.hash = None;
            entry.copy = Some(level.as_str().to_string());
        } else {
            // An unreadable original is sent as is.
            left.push((src, file));
        }
        let _ = progress.send(TransferProgress {
            fraction: 0.5 * (i + 1) as f32 / total as f32,
        });
    }
    manifest.write(out).map_err(|_| fail("transfer.manifest"))?;
    Ok(left)
}

/// «Отключить телефон»: forgets the phone and, if no other phone uses it, its Apple ID (D-132).
#[tauri::command(async)]
#[specta::specta]
pub fn phone_forget(hub: State<'_, PhoneHub>, udid: String) -> Result<(), AppError> {
    let Some(r) = hub.forget(&udid) else {
        return Ok(());
    };
    sirtone_devicelink::wifi::forget(&udid).map_err(|e| app_error(&e))?;
    let shared = hub
        .records
        .lock()
        .is_ok_and(|all| all.values().any(|o| o.apple_id == r.apple_id));
    if !shared {
        apple::forget(&r.apple_id).map_err(|e| app_error(&e))?;
    }
    Ok(())
}

#[cfg(test)]
mod copy_package_tests {
    use super::*;
    use sirtone_library::repo::{Identity, NewTrack, artist_get_or_create, track_insert};

    fn wav(path: &std::path::Path) {
        let (rate, frames) = (44_100u32, 44_100u32);
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + frames * 2).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(frames * 2).to_le_bytes());
        for i in 0..frames {
            b.extend_from_slice(&(((i as f32 * 0.05).sin() * 7_000.0) as i16).to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    /// Copies go into the package and the manifest says so; a repeat finds them there (nothing
    /// «missing» from the library); another quality uses its own folder.
    #[test]
    fn copies_into_the_package_resume_cleanly() {
        let base = std::env::temp_dir().join(format!("sirtone-hub-copies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("lib");
        std::fs::create_dir_all(root.join("Кино")).unwrap();
        wav(&root.join("Кино").join("01 Ёлка.wav"));
        let mut c = sirtone_library::open(
            &root.join(".sirtone/library.db"),
            sirtone_library::Medium::Fixed,
        )
        .unwrap();
        let kino = artist_get_or_create(&c, "Кино").unwrap();
        track_insert(
            &mut c,
            &NewTrack {
                rel_path: "Кино/01 Ёлка.wav".into(),
                size_bytes: 88_244,
                mtime_ms: 0,
                content_hash: "h".into(),
                container: "wav".into(),
                codec: "pcm_s16le".into(),
                lossless: true,
                sample_rate: Some(44_100),
                channels: Some(1),
                duration_ms: 1_000,
                title: "Ёлка".into(),
                album_id: None,
                disc_no: None,
                track_no: Some(1),
                year: None,
                identity: Identity::Inferred,
                identity_conf: 0.9,
                unknown_reason: None,
                artists: vec![kino],
            },
        )
        .unwrap();
        let parts = sirtone_transfer::Parts {
            music: true,
            playlists: true,
            taste: true,
            settings: false,
        };
        let progress: Channel<TransferProgress> = Channel::new(|_| Ok(()));
        let level = sirtone_audio::encode::Level::Economy;
        let out = base.join("out").join(level.as_str());
        for round in 0..2 {
            sirtone_sync::export_with(
                &c,
                &root,
                &out,
                sirtone_transfer::Platform::Windows,
                "t",
                parts,
                false,
            )
            .unwrap();
            let music = sirtone_sync::music_sources(&root, &out).unwrap();
            assert_eq!(music.len(), if round == 0 { 1 } else { 0 }, "round {round}");
            let jobs = music.iter().map(|_| None).collect();
            let left = encode_package(&out, music, jobs, level, &progress).unwrap();
            assert!(left.is_empty());
            let m = sirtone_transfer::Manifest::read(&out).unwrap();
            assert_eq!(m.tracks[0].copy.as_deref(), Some("economy"));
            assert!(m.tracks[0].file.ends_with(".opus"));
            let copy = m.tracks[0]
                .file
                .split('/')
                .fold(out.clone(), |p, s| p.join(s));
            assert!(copy.exists());
            assert!(m.tracks[0].size > 0 && m.tracks[0].size < 88_244);
        }
        // «Оригинал» afterwards: its own folder, the original music, no copy marks.
        let orig = base.join("out").join("original");
        sirtone_sync::export_with(
            &c,
            &root,
            &orig,
            sirtone_transfer::Platform::Windows,
            "t",
            parts,
            false,
        )
        .unwrap();
        let music = sirtone_sync::music_sources(&root, &orig).unwrap();
        assert_eq!(music.len(), 1);
        assert!(music[0].0.exists());
        let m = sirtone_transfer::Manifest::read(&orig).unwrap();
        assert!(m.tracks[0].copy.is_none());
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod renew_tests {
    use super::*;

    #[test]
    fn renews_inside_the_three_day_window_only() {
        let now = 1_800_000_000_000u64;
        let day = 24 * 3_600_000;
        let rec = |until: Option<u64>, installed: Option<u64>| PhoneRecord {
            apple_id: "a@b.c".into(),
            installed_at_ms: installed,
            works_until_ms: until,
            ..PhoneRecord::default()
        };
        let mut all = BTreeMap::new();
        all.insert("far".to_string(), rec(Some(now + 5 * day), None));
        all.insert("near".to_string(), rec(Some(now + 2 * day), None));
        all.insert("old-install".to_string(), rec(None, Some(now - 6 * day)));
        all.insert("never".to_string(), rec(None, None));
        let mut failed = rec(Some(now + day), None);
        failed.renew_failed_ms = Some(now - 3_600_000);
        all.insert("failed-just-now".to_string(), failed);
        let ids: Vec<String> = due(&all, now).into_iter().map(|(u, _)| u).collect();
        assert_eq!(ids, ["near", "old-install"]);
    }
}
