//! П4б (docs/mobile/checks.md): install over Wi-Fi on iOS 17+ through RemotePairing → TLS-PSK
//! CDTunnel → userspace TCP (jktcp) → RSD, then AFC + installation_proxy inside the tunnel.
//!
//! The sequence is copied from idevice's own tools (jkcoxson/idevice `tools/src/rppairing.rs`,
//! `remotepairing_lockdown.rs`, `pair_rsd_ios.rs`; `01d` [M8]):
//!
//! sp4 rsd-usb <apple-id> <App.app>          — the same RSD install path over the cable
//!                                             (CoreDeviceProxy): proves install inside a tunnel
//! sp4 rp-pair                                — once, cable: RPPairing record via
//!                                             `com.apple.dt.remotepairingdeviced.lockdown`
//!                                             (promptless on a trusted cable), kept in the hub's
//!                                             Credential Manager store — never on disk (Ed25519 key)
//! sp4 rp-wifi <apple-id> <udid> <App.app|-> <addr:port>   (addr:port — `sirtone-cli phone wifi-find`)
//!                                            — Wi-Fi: pair-verify (no fallback to a new pairing),
//!                                             tunnel, RSD; with an app — sign and install
//!
//! Nothing personal reaches the output: service counts, ports and timings only.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use idevice::IdeviceService;
use idevice::core_device_proxy::CoreDeviceProxy;
use idevice::lockdown::LockdownClient;
use idevice::provider::IdeviceProvider;
use idevice::remote_pairing::{
    RemotePairingClient, RemotePairingLockdownService, RpPairingFile, RpPairingSocket,
    connect_tls_psk_tunnel_native,
};
use idevice::rsd::RsdHandshake;
use idevice::tcp::adapter::Adapter;
use idevice::tcp::handle::AdapterHandle;
use idevice::usbmuxd::{UsbmuxdAddr, UsbmuxdConnection};
use isideload::dev::devices::DevicesApi;
use isideload::sideload::builder::MaxCertsBehavior;
use isideload::sideload::{SideloaderBuilder, TeamSelection};
use isideload::util::callbacks::{ExtensionsBehaviorCallbackBox, MaxCertsCallbackBox};
use isideload::util::storage::SideloadingStorage;

use crate::{MACHINE, session, short, storage};

/// The hub's store for RPPairing records (`sirtone-devicelink::wifi`): one record per phone for
/// both, or pairing from one side would replace the other's entry on the phone.
const RP_STORE: &str = "studio.serkov.sirtone.rppairing";

fn rp_store() -> isideload::util::keyring_storage::KeyringStorage {
    isideload::util::keyring_storage::KeyringStorage::new(RP_STORE.to_string())
}

async fn usb_provider() -> Result<(idevice::usbmuxd::UsbmuxdDevice, impl IdeviceProvider), String> {
    let mut mux = UsbmuxdConnection::default()
        .await
        .map_err(|e| format!("usbmuxd: {e:?}"))?;
    let devices = mux.get_devices().await.map_err(|e| format!("usbmuxd: {e:?}"))?;
    let device = devices
        .into_iter()
        .find(|d| matches!(d.connection_type, idevice::usbmuxd::Connection::Usb))
        .ok_or("device.none: подключите iPhone кабелем")?;
    let addr = UsbmuxdAddr::from_env_var().map_err(|e| format!("usbmuxd addr: {e:?}"))?;
    let provider = device.to_provider(addr, "sirtone-p4");
    Ok((device, provider))
}

async fn device_name(provider: &impl IdeviceProvider) -> Result<String, String> {
    let mut lockdown = LockdownClient::connect(provider)
        .await
        .map_err(|e| format!("lockdown: {e:?}"))?;
    let v = lockdown
        .get_value(Some("DeviceName"), None)
        .await
        .map_err(|e| format!("lockdown get: {e:?}"))?;
    Ok(v.as_string().unwrap_or("iPhone").to_string())
}

/// RSD over a running userspace TCP stack; prints only counts and the iOS version.
async fn rsd(handle: &mut AdapterHandle, port: u16) -> Result<RsdHandshake, String> {
    let s = handle.connect(port).await.map_err(|e| format!("rsd connect: {e:?}"))?;
    let hs = RsdHandshake::new(s).await.map_err(|e| format!("rsd handshake: {e:?}"))?;
    let ver = hs
        .properties
        .get("OSVersion")
        .and_then(|v| v.as_string())
        .unwrap_or("?");
    println!("  RSD: {} служб, iOS {ver}", hs.services.len());
    for k in ["com.apple.afc.shim.remote", "com.apple.mobile.installation_proxy.shim.remote"] {
        println!("  {k}: {}", if hs.services.contains_key(k) { "есть" } else { "НЕТ" });
    }
    Ok(hs)
}

/// isideload signs the bundle in place and deletes it after install (П4 §4): work on a copy.
fn temp_copy(app: &Path) -> Result<PathBuf, String> {
    fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            let t = to.join(e.file_name());
            if e.file_type()?.is_dir() {
                copy(&e.path(), &t)?;
            } else {
                std::fs::copy(e.path(), t)?;
            }
        }
        Ok(())
    }
    let name = app.file_name().ok_or("app path")?;
    let dir = std::env::temp_dir().join(format!("sirtone-p4b-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let dst = dir.join(name);
    copy(app, &dst).map_err(|e| format!("copy app: {e}"))?;
    Ok(dst)
}

/// Register the device (as `Sideloader::install_app` does), sign a copy, install through RSD.
async fn sign_and_install_rsd(
    apple_id: &str,
    udid: &str,
    name: &str,
    app: &Path,
    handle: &mut AdapterHandle,
    hs: &mut RsdHandshake,
) -> Result<(), String> {
    let dev = session(apple_id).await?;
    let mut sideloader =
        SideloaderBuilder::<MaxCertsCallbackBox, ExtensionsBehaviorCallbackBox>::new(dev, apple_id.to_string())
            .team_selection(TeamSelection::First)
            .max_certs_behavior(MaxCertsBehavior::Error)
            .storage(Box::new(storage()))
            .machine_name(MACHINE.to_string())
            .build();
    let t0 = Instant::now();
    let team = sideloader.get_team().await.map_err(|e| format!("team: {}", short(&e)))?;
    sideloader
        .get_dev_session()
        .ensure_device_registered(&team, name, udid, None)
        .await
        .map_err(|e| format!("device register: {}", short(&e)))?;
    let copy = temp_copy(app)?;
    let (signed, _) = sideloader
        .sign_app(copy, Some(team), false, None::<fn(f32) -> std::future::Ready<()>>, None, Some(udid))
        .await
        .map_err(|e| format!("sign: {}", short(&e)))?;
    let t_sign = t0.elapsed();
    let last = std::sync::atomic::AtomicU64::new(0);
    isideload::sideload::install::install_app_rsd(handle, hs, &signed, |p| {
        if p >= last.load(std::sync::atomic::Ordering::Relaxed) + 25 {
            last.store(p, std::sync::atomic::Ordering::Relaxed);
            println!("  установка: {p}%");
        }
    })
    .await
    .map_err(|e| {
        if std::env::var_os("SP4_FULL_ERR").is_some() {
            eprintln!("{e}");
        }
        format!("install: {}", short(&e))
    })?;
    if let Some(parent) = signed.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
    println!(
        "Установлено за {:.1} с (подпись {:.1} с).",
        t0.elapsed().as_secs_f32(),
        t_sign.as_secs_f32()
    );
    Ok(())
}

/// The RSD install path over the cable: CoreDeviceProxy → CDTunnel → jktcp → RSD.
pub async fn rsd_usb(apple_id: &str, app: PathBuf) -> Result<(), String> {
    let (device, provider) = usb_provider().await?;
    let name = device_name(&provider).await?;
    let t0 = Instant::now();
    let proxy = CoreDeviceProxy::connect(&provider)
        .await
        .map_err(|e| format!("core device proxy: {e:?}"))?;
    let rsd_port = proxy.tunnel_info().server_rsd_port;
    let mut handle = proxy
        .create_software_tunnel()
        .map_err(|e| format!("tunnel: {e:?}"))?;
    window(&mut handle);
    let mut handle = handle.to_async_handle();
    println!("Туннель по кабелю за {:.2} с.", t0.elapsed().as_secs_f32());
    let mut hs = rsd(&mut handle, rsd_port).await?;
    sign_and_install_rsd(apple_id, &device.udid, &name, &app, &mut handle, &mut hs).await
}

/// Once over a trusted cable: RPPairing record for this PC, kept in Credential Manager.
pub async fn rp_pair() -> Result<(), String> {
    let (device, provider) = usb_provider().await?;
    let service = RemotePairingLockdownService::connect(&provider)
        .await
        .map_err(|e| format!("remotepairingdeviced: {e:?}"))?;
    let mut client = service.into_client(MACHINE).map_err(|e| format!("socket: {e:?}"))?;
    let mut rpf = RpPairingFile::generate(MACHINE);
    let t0 = Instant::now();
    println!("Сопряжение… (если на iPhone спросит — «Доверять»)");
    client
        .connect(&mut rpf, async || "000000".to_string())
        .await
        .map_err(|e| format!("rp pair: {e:?}"))?;
    let st = rp_store();
    st.store_data(&device.udid, &rpf.to_bytes())
        .map_err(|e| format!("store: {}", short(&e)))?;
    let mut index: Vec<String> = st
        .retrieve("index")
        .ok()
        .flatten()
        .map(|v| v.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    if !index.contains(&device.udid) {
        index.push(device.udid.clone());
        st.store("index", &index.join(","))
            .map_err(|e| format!("store: {}", short(&e)))?;
    }
    println!(
        "Сопряжено за {:.1} с; запись — в диспетчере учётных данных (alt_irk: {}).",
        t0.elapsed().as_secs_f32(),
        if rpf.alt_irk().is_some() { "есть" } else { "нет" }
    );
    Ok(())
}

fn load_rp(udid: &str) -> Result<RpPairingFile, String> {
    let bytes = rp_store()
        .retrieve_data(udid)
        .map_err(|e| format!("keyring: {}", short(&e)))?
        .filter(|b| !b.is_empty())
        .ok_or("rp.no_record: сначала `sirtone-cli phone wifi-pair usb` (или `sp4 rp-pair`) по кабелю")?;
    RpPairingFile::from_bytes(&bytes).map_err(|e| format!("rp record: {e:?}"))
}

async fn connect_any(addrs: &[IpAddr], port: u16) -> Result<(tokio::net::TcpStream, IpAddr), String> {
    let mut last = String::from("no address");
    for a in addrs {
        // Link-local IPv6 needs a scope; mDNS gives none here, so v4 goes first (browse sorts).
        let sa = SocketAddr::new(*a, port);
        match tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect(sa)).await {
            Ok(Ok(s)) => return Ok((s, *a)),
            Ok(Err(e)) => last = format!("{e}"),
            Err(_) => last = "timeout".into(),
        }
    }
    Err(format!("connect: {last}"))
}

/// Wi-Fi: pair-verify with the stored record, CDTunnel, RSD; with an app — sign and install.
pub async fn rp_wifi(apple_id: &str, udid: &str, app: Option<PathBuf>, at: Option<&str>) -> Result<(), String> {
    let mut rpf = load_rp(udid)?;
    let t0 = Instant::now();
    // Discovery is the hub's (`sirtone-cli phone wifi-find`, Windows DNS-SD): mdns-sd heard
    // nothing on this PC (spike-P-04b §4).
    let sa: SocketAddr = at
        .ok_or("usage: … <addr:port> из `sirtone-cli phone wifi-find`")?
        .parse()
        .map_err(|_| "addr:port")?;
    let (addrs, port) = (vec![sa.ip()], sa.port());
    let (stream, ip) = connect_any(&addrs, port).await?;
    let mut rpc = RemotePairingClient::new(RpPairingSocket::new(stream), MACHINE);
    rpc.attempt_pair_verify().await.map_err(|e| format!("rp handshake: {e:?}"))?;
    // No fallback to a fresh pairing here: a broken record must show, not prompt on the phone.
    rpc.validate_pairing(&mut rpf)
        .await
        .map_err(|e| format!("rp verify: {e:?}"))?;
    let tport = rpc.create_tcp_listener().await.map_err(|e| format!("listener: {e:?}"))?;
    let ts = tokio::net::TcpStream::connect(SocketAddr::new(ip, tport))
        .await
        .map_err(|e| format!("tunnel connect: {e}"))?;
    let tunnel = connect_tls_psk_tunnel_native(ts, rpc.encryption_key())
        .await
        .map_err(|e| format!("tls-psk: {e:?}"))?;
    let info = tunnel.info.clone();
    let client_ip: IpAddr = info.client_address.parse().map_err(|_| "client ip")?;
    let server_ip: IpAddr = info.server_address.parse().map_err(|_| "server ip")?;
    let mut adapter = Adapter::new(Box::new(tunnel.into_inner()), client_ip, server_ip);
    adapter.set_mss((info.mtu as usize).saturating_sub(60));
    window(&mut adapter);
    let mut handle = adapter.to_async_handle();
    println!("Туннель по Wi-Fi за {:.1} с (MTU {}).", t0.elapsed().as_secs_f32(), info.mtu);
    let mut hs = rsd(&mut handle, info.server_rsd_port).await?;
    if let Some(app) = app {
        let name = hs
            .properties
            .get("DeviceName")
            .and_then(|v| v.as_string())
            .unwrap_or("iPhone")
            .to_string();
        sign_and_install_rsd(apple_id, udid, &name, &app, &mut handle, &mut hs).await?;
    }
    drop(rpc);
    println!("Всего {:.1} с.", t0.elapsed().as_secs_f32());
    Ok(())
}

/// Experiment: bytes in flight per connection (`SP4_WINDOW`), jktcp's default is 1 MiB.
fn window(a: &mut idevice::tcp::adapter::Adapter) {
    if let Some(w) = std::env::var("SP4_WINDOW").ok().and_then(|v| v.parse().ok()) {
        a.set_send_window(w);
        println!("  окно отправки: {w} байт");
    }
    if let Some(m) = std::env::var("SP4_MSS").ok().and_then(|v| v.parse().ok()) {
        a.set_mss(m);
        println!("  MSS: {m} байт");
    }
}
