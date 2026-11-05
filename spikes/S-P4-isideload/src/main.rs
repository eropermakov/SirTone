//! П4 spike (docs/mobile/checks.md): sign and install the iPhone app from Windows with the owner's
//! Apple ID — `isideload` (Apple developer services, signing) + `idevice` (usbmuxd, AFC,
//! installation proxy) — and check that the hub can renew **without the password** (D-132: the
//! password is never stored; the developer-services token and adsid are, in Windows Credential
//! Manager).
//!
//! sp4 login   <apple-id>             — once, in the owner's own terminal: password and 2FA code
//! sp4 info    <apple-id>             — team, certificates, App IDs (no password)
//! sp4 install <apple-id> <App.app>   — sign + install over the cable (no password)
//!
//! Safety: certificates are never revoked (MaxCertsBehavior::Error) — revoking would stop apps
//! already signed with them. Secrets never reach the log: isideload is held at WARN and its
//! debug mode stays off.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use idevice::usbmuxd::{UsbmuxdAddr, UsbmuxdConnection};
use isideload::anisette::remote_v3::RemoteV3AnisetteProvider;
use isideload::auth::apple_account::{
    AppToken, AppleAccount, TwoFactorCallbackParams, TwoFactorCallbackResponse,
};
use isideload::dev::app_ids::AppIdsApi;
use isideload::dev::certificates::CertificatesApi;
use isideload::dev::developer_session::DeveloperSession;
use isideload::dev::device_type::DeveloperDeviceType;
use isideload::dev::teams::TeamsApi;
use isideload::sideload::builder::MaxCertsBehavior;
use isideload::sideload::{SideloaderBuilder, TeamSelection};
use isideload::util::callbacks::{ExtensionsBehaviorCallbackBox, MaxCertsCallbackBox};
use isideload::util::keyring_storage::KeyringStorage;
use isideload::util::storage::SideloadingStorage;

/// Credential Manager service for everything this spike keeps (token, adsid, anisette state,
/// signing key). The real hub uses its own name per account (D-132).
const SERVICE: &str = "sirtone-p4";
const MACHINE: &str = "SirTone-PC";

fn storage() -> KeyringStorage {
    KeyringStorage::new(SERVICE.to_string())
}

fn anisette() -> RemoteV3AnisetteProvider {
    // Anisette state (the provisioned «device») lives in Credential Manager too, so Apple sees
    // the same machine every time and 2FA is asked once.
    RemoteV3AnisetteProvider::default()
        .expect("anisette provider")
        .set_storage(Box::new(storage()))
        .set_serial_number("2".to_string())
}

/// The first line of an error report only: attachments may carry Apple plists (D-132).
fn short(e: &rootcause::Report) -> String {
    e.to_string().lines().next().unwrap_or("").to_string()
}

fn line(prompt: &str) -> String {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    s.trim().to_string()
}

async fn two_factor(p: TwoFactorCallbackParams) -> Result<TwoFactorCallbackResponse, rootcause::Report> {
    if p.unknown {
        println!("Код не подошёл или способ не сработал.");
    }
    if p.sms {
        println!("Код отправлен по SMS.");
    } else {
        println!("Код показан на ваших устройствах Apple.");
    }
    let code = line("Введите 6 цифр (или «d» — прислать на устройства, «r» — ещё раз): ");
    Ok(match code.as_str() {
        "d" => TwoFactorCallbackResponse::SendToDevices,
        "r" => TwoFactorCallbackResponse::ResendCode,
        "" => TwoFactorCallbackResponse::Abort,
        c => TwoFactorCallbackResponse::SubmitCode(c.to_string()),
    })
}

async fn login(apple_id: &str) -> Result<(), String> {
    if !std::io::stdin().is_terminal() {
        // Never hang on a missing console: the password is typed by the owner only.
        return Err("login.no_console".into());
    }
    let password = rpassword::prompt_password(format!("Пароль Apple ID {apple_id}: "))
        .map_err(|_| "login.no_console".to_string())?;
    let mut account = AppleAccount::builder(apple_id)
        .anisette_provider(anisette())
        .login(&password, two_factor)
        .await
        .map_err(|e| format!("login.failed: {}", short(&e)))?;
    drop(password);
    let token = account
        .get_app_token("xcode.auth")
        .await
        .map_err(|e| format!("login.token: {}", short(&e)))?;
    let adsid = account
        .spd
        .as_ref()
        .and_then(|d| d.get("adsid"))
        .and_then(|v| v.as_string())
        .ok_or("login.adsid")?
        .to_string();
    let s = storage();
    let key = |k: &str| format!("{apple_id}:{k}");
    s.store(&key("token"), &token.token).map_err(|_| "store")?;
    s.store(&key("token_expiry"), &token.expiry.to_string()).map_err(|_| "store")?;
    s.store(&key("token_duration"), &token.duration.to_string()).map_err(|_| "store")?;
    s.store(&key("adsid"), &adsid).map_err(|_| "store")?;
    println!("Вход выполнен. Токен сервисов разработчика действует до {} (unix ms).", token.expiry);
    Ok(())
}

/// A developer session from the stored token: no password, no 2FA.
async fn session(apple_id: &str) -> Result<DeveloperSession, String> {
    let s = storage();
    let get = |k: &str| {
        s.retrieve(&format!("{apple_id}:{k}"))
            .ok()
            .flatten()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| "session.no_token: сначала `sp4 login`".to_string())
    };
    let token = AppToken {
        token: get("token")?,
        expiry: get("token_expiry")?.parse().unwrap_or(0),
        duration: get("token_duration")?.parse().unwrap_or(0),
    };
    let adsid = get("adsid")?;
    // An account object without login: it builds the GrandSlam client and anisette generator.
    let account = AppleAccount::builder(apple_id)
        .anisette_provider(anisette())
        .build()
        .await
        .map_err(|e| format!("session.client: {}", short(&e)))?;
    Ok(DeveloperSession::new(
        token,
        adsid,
        account.grandslam_client.clone(),
        account.anisette_generator.clone(),
    ))
}

async fn info(apple_id: &str) -> Result<(), String> {
    let mut dev = session(apple_id).await?;
    let teams = dev.list_teams().await.map_err(|e| format!("teams: {}", short(&e)))?;
    for t in &teams {
        println!("Команда {} ({:?}, {:?})", t.team_id, t.r#type, t.status);
        let certs = dev
            .list_all_development_certs(t, DeveloperDeviceType::Ios)
            .await
            .map_err(|e| format!("certs: {}", short(&e)))?;
        for c in certs {
            println!(
                "  сертификат: {:?} / машина {:?} / до {:?}",
                c.name, c.machine_name, c.expiration_date
            );
        }
        let ids = dev
            .list_app_ids(t, DeveloperDeviceType::Ios)
            .await
            .map_err(|e| format!("app ids: {}", short(&e)))?;
        println!(
            "  App ID: {} (доступно ещё {:?} из {:?})",
            ids.app_ids.len(),
            ids.available_quantity,
            ids.max_quantity
        );
        for a in ids.app_ids {
            println!("    {} — до {:?}", a.identifier, a.expiration_date);
        }
    }
    Ok(())
}

async fn install(apple_id: &str, app: PathBuf) -> Result<(), String> {
    let dev = session(apple_id).await?;
    let mut mux = UsbmuxdConnection::default()
        .await
        .map_err(|e| format!("usbmuxd: {e:?}"))?;
    let devices = mux.get_devices().await.map_err(|e| format!("usbmuxd: {e:?}"))?;
    let device = devices.first().ok_or("device.none")?;
    println!("Телефон: {} ({:?})", device.udid, device.connection_type);
    let addr = UsbmuxdAddr::from_env_var().map_err(|e| format!("usbmuxd addr: {e:?}"))?;
    let provider = device.to_provider(addr, "sirtone-p4");

    let mut sideloader =
        SideloaderBuilder::<MaxCertsCallbackBox, ExtensionsBehaviorCallbackBox>::new(dev, apple_id.to_string())
            .team_selection(TeamSelection::First)
            .max_certs_behavior(MaxCertsBehavior::Error)
            .storage(Box::new(storage()))
            .machine_name(MACHINE.to_string())
            .build();
    let started = std::time::Instant::now();
    sideloader
        .install_app(
            &provider,
            app,
            false,
            Some(|p: f32| async move {
                println!("  подпись: {:.0}%", p * 100.0);
            }),
            None,
        )
        .await
        .map_err(|e| {
            // Full report only on request, on this PC: it may carry Apple plists.
            if std::env::var_os("SP4_FULL_ERR").is_some() {
                eprintln!("{e}");
            }
            format!("install: {}", short(&e))
        })?;
    println!("Установлено за {:.1} с.", started.elapsed().as_secs_f32());
    Ok(())
}

/// Lets the phone talk to this PC over Wi-Fi (the switch iTunes calls «Sync over Wi-Fi»):
/// lockdown `com.apple.mobile.wireless_lockdown` / `EnableWifiConnections`. Needs the cable once.
async fn wifi(enable: bool) -> Result<(), String> {
    use idevice::IdeviceService;
    use idevice::lockdown::LockdownClient;
    use idevice::provider::IdeviceProvider;
    const DOMAIN: &str = "com.apple.mobile.wireless_lockdown";
    let mut mux = UsbmuxdConnection::default()
        .await
        .map_err(|e| format!("usbmuxd: {e:?}"))?;
    let devices = mux.get_devices().await.map_err(|e| format!("usbmuxd: {e:?}"))?;
    for d in &devices {
        println!("Телефон: {} ({:?})", d.udid, d.connection_type);
    }
    let device = devices.first().ok_or("device.none")?;
    let addr = UsbmuxdAddr::from_env_var().map_err(|e| format!("usbmuxd addr: {e:?}"))?;
    let provider = device.to_provider(addr, "sirtone-p4");
    let mut lockdown = LockdownClient::connect(&provider)
        .await
        .map_err(|e| format!("lockdown: {e:?}"))?;
    let pairing = provider
        .get_pairing_file()
        .await
        .map_err(|e| format!("pairing: {e:?}"))?;
    lockdown
        .start_session(&pairing)
        .await
        .map_err(|e| format!("session: {e:?}"))?;
    let before = lockdown
        .get_value(Some("EnableWifiConnections"), Some(DOMAIN))
        .await
        .map_err(|e| format!("get: {e:?}"))?;
    println!("Было: {before:?}");
    lockdown
        .set_value("EnableWifiConnections", plist::Value::Boolean(enable), Some(DOMAIN))
        .await
        .map_err(|e| format!("set: {e:?}"))?;
    let after = lockdown
        .get_value(Some("EnableWifiConnections"), Some(DOMAIN))
        .await
        .map_err(|e| format!("get: {e:?}"))?;
    println!("Стало: {after:?}");
    Ok(())
}

/// A direct network link to the phone (lockdown on TCP 62078), with the pairing record the Apple
/// service keeps: Wi-Fi without the service's own network support.
async fn net_provider(ip: &str, udid: &str) -> Result<idevice::provider::TcpProvider, String> {
    let mut mux = UsbmuxdConnection::default()
        .await
        .map_err(|e| format!("usbmuxd: {e:?}"))?;
    let pairing_file = mux
        .get_pair_record(udid)
        .await
        .map_err(|e| format!("pair record: {e:?}"))?;
    Ok(idevice::provider::TcpProvider {
        addr: ip.parse().map_err(|_| "ip")?,
        scope_id: None,
        pairing_file,
        label: "sirtone-p4".into(),
    })
}

async fn net_info(ip: &str, udid: &str) -> Result<(), String> {
    use idevice::IdeviceService;
    use idevice::lockdown::LockdownClient;
    use idevice::provider::IdeviceProvider;
    let provider = net_provider(ip, udid).await?;
    let mut lockdown = LockdownClient::connect(&provider)
        .await
        .map_err(|e| format!("lockdown: {e:?}"))?;
    let pairing = provider.get_pairing_file().await.map_err(|e| format!("{e:?}"))?;
    lockdown
        .start_session(&pairing)
        .await
        .map_err(|e| format!("session: {e:?}"))?;
    for k in ["DeviceName", "ProductType", "ProductVersion"] {
        let v = lockdown.get_value(Some(k), None).await.map_err(|e| format!("{e:?}"))?;
        println!("{k}: {v:?}");
    }
    Ok(())
}

async fn net_afc(ip: &str, udid: &str) -> Result<(), String> {
    use idevice::IdeviceService;
    use idevice::afc::AfcClient;
    let provider = net_provider(ip, udid).await?;
    let mut afc = AfcClient::connect(&provider)
        .await
        .map_err(|e| format!("afc connect: {e:?}"))?;
    let list = afc.list_dir("/").await.map_err(|e| format!("afc list: {e:?}"))?;
    println!("AFC по сети: {} записей в корне", list.len());
    Ok(())
}

async fn net_install(apple_id: &str, ip: &str, udid: &str, app: PathBuf) -> Result<(), String> {
    let dev = session(apple_id).await?;
    let provider = net_provider(ip, udid).await?;
    let mut sideloader =
        SideloaderBuilder::<MaxCertsCallbackBox, ExtensionsBehaviorCallbackBox>::new(dev, apple_id.to_string())
            .team_selection(TeamSelection::First)
            .max_certs_behavior(MaxCertsBehavior::Error)
            .storage(Box::new(storage()))
            .machine_name(MACHINE.to_string())
            .build();
    let started = std::time::Instant::now();
    sideloader
        .install_app(&provider, app, false, None::<fn(f32) -> std::future::Ready<()>>, None)
        .await
        .map_err(|e| {
            if std::env::var_os("SP4_FULL_ERR").is_some() {
                eprintln!("{e}");
            }
            format!("install: {}", short(&e))
        })?;
    println!("Установлено по сети за {:.1} с.", started.elapsed().as_secs_f32());
    Ok(())
}

/// Phones the Apple service sees now (cable and Wi-Fi).
async fn list() -> Result<(), String> {
    let mut mux = UsbmuxdConnection::default()
        .await
        .map_err(|e| format!("usbmuxd: {e:?}"))?;
    for d in mux.get_devices().await.map_err(|e| format!("usbmuxd: {e:?}"))? {
        println!("{} {:?}", d.udid, d.connection_type);
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let _ = isideload::init();
    // isideload at WARN: its INFO/DEBUG lines are progress, but errors may carry plists.
    tracing_subscriber::fmt()
        .with_env_filter("warn,apple_codesign=error,sp4_isideload=info")
        .init();

    let args: Vec<String> = std::env::args().collect();
    let r = match (args.get(1).map(String::as_str), args.get(2), args.get(3)) {
        (Some("login"), Some(id), _) => login(id).await,
        (Some("info"), Some(id), _) => info(id).await,
        (Some("install"), Some(id), Some(app)) => install(id, PathBuf::from(app)).await,
        (Some("wifi"), Some(v), _) => wifi(v == "on").await,
        (Some("list"), _, _) => list().await,
        (Some("net-info"), Some(ip), Some(udid)) => net_info(ip, udid).await,
        (Some("net-afc"), Some(ip), Some(udid)) => net_afc(ip, udid).await,
        (Some("net-install"), Some(id), Some(ip)) => match (args.get(4), args.get(5)) {
            (Some(udid), Some(app)) => net_install(id, ip, udid, PathBuf::from(app)).await,
            _ => Err("usage: sp4 net-install <apple-id> <ip> <udid> <App.app>".into()),
        },
        _ => Err("usage: sp4 login|info <apple-id> | sp4 install <apple-id> <App.app>".into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ошибка: {e}");
            ExitCode::FAILURE
        }
    }
}
