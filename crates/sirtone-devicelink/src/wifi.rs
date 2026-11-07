//! iPhone over Wi-Fi (check П4б; shared with self-renewal П9): RemotePairing instead of the Apple
//! service's network mode, which iOS 27 no longer serves for AFC (`docs/reports/spike-P-04.md`).
//!
//! The sequence is idevice's own (`01d` [M8a]):
//! 1. **Once, on a trusted cable** — [`pair_over_cable`]: an RPPairing record for this PC through
//!    `com.apple.dt.remotepairingdeviced.lockdown` (no prompt: the cable is already trusted). The
//!    record holds an Ed25519 private key, so it lives only in Windows Credential Manager.
//! 2. **Then over Wi-Fi** — [`find`] browses Bonjour `_remotepairing._tcp` and recognises our
//!    phones by `authTag` (keyed with the record's `alt_irk`, as CoreDevice does); [`open`]
//!    pair-verifies with the record (never a fresh pairing: a broken record must show, not
//!    prompt on the phone), asks for a tunnel listener, does TLS-PSK + CDTunnel, runs the
//!    userspace TCP stack (jktcp) and the RSD handshake. AFC and installation_proxy then go
//!    through the tunnel (`apple::install`).
//!
//! Nothing personal reaches logs or errors: debug codes and counts only.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use idevice::IdeviceService;
use idevice::remote_pairing::{
    PeerDevice, RemotePairingClient, RemotePairingLockdownService, RpPairingFile, RpPairingSocket,
    connect_tls_psk_tunnel_native,
};
use idevice::rsd::RsdHandshake;
use idevice::tcp::adapter::Adapter;
use idevice::tcp::handle::AdapterHandle;
use idevice::usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdConnection};
use isideload::util::keyring_storage::KeyringStorage;
use isideload::util::storage::SideloadingStorage;

use crate::LinkError;

/// Credential Manager service for RPPairing records, one key per phone (UDID).
const SERVICE: &str = "studio.serkov.sirtone.rppairing";
/// The phone knows this PC by it (its uuid-v3 is the record's identifier): never change it, or
/// every phone gets a second pairing entry.
const HOST: &str = "SirTone-PC";
const INDEX: &str = "index";
const BONJOUR: &str = "_remotepairing._tcp.local.";

fn err(code: &'static str) -> impl Fn(rootcause::Report) -> LinkError {
    move |e| LinkError::Apple(code, crate::apple::short(&e))
}

fn keyring() -> KeyringStorage {
    KeyringStorage::new(SERVICE.to_string())
}

fn ids(s: &dyn SideloadingStorage) -> Vec<String> {
    s.retrieve(INDEX)
        .ok()
        .flatten()
        .map(|v| {
            v.split(',')
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn save(s: &dyn SideloadingStorage, udid: &str, rec: &RpPairingFile) -> Result<(), LinkError> {
    s.store_data(udid, &rec.to_bytes())
        .map_err(err("phone.wifi_store"))?;
    let mut all = ids(s);
    if !all.iter().any(|x| x == udid) {
        all.push(udid.to_string());
        s.store(INDEX, &all.join(","))
            .map_err(err("phone.wifi_store"))?;
    }
    Ok(())
}

fn load(s: &dyn SideloadingStorage, udid: &str) -> Option<RpPairingFile> {
    let bytes = s.retrieve_data(udid).ok().flatten()?;
    if bytes.is_empty() {
        return None;
    }
    RpPairingFile::from_bytes(&bytes).ok()
}

fn remove(s: &dyn SideloadingStorage, udid: &str) -> Result<(), LinkError> {
    s.delete(udid).map_err(err("phone.wifi_store"))?;
    let rest: Vec<String> = ids(s).into_iter().filter(|x| x != udid).collect();
    s.store(INDEX, &rest.join(","))
        .map_err(err("phone.wifi_store"))
}

/// Which of our phones a Bonjour `_remotepairing._tcp` service is (its `identifier` and
/// `authTag` TXT values against each record's `alt_irk`).
fn recognise<'a>(
    records: &'a [(String, RpPairingFile)],
    identifier: &str,
    auth_tag: &str,
) -> Option<&'a str> {
    records.iter().find_map(|(udid, r)| {
        r.alt_irk()
            .filter(|k| PeerDevice::validate_auth_tag(k, identifier, auth_tag))
            .map(|_| udid.as_str())
    })
}

/// Whether this phone can be reached over Wi-Fi (a record exists; no network).
pub fn is_paired(udid: &str) -> bool {
    load(&keyring(), udid).is_some()
}

/// Forgets the phone's Wi-Fi record («Отключить телефон»).
pub fn forget(udid: &str) -> Result<(), LinkError> {
    remove(&keyring(), udid)
}

/// Once, with the phone on a trusted cable: makes and keeps the RPPairing record (wizard step 10,
/// «Включает доступ по Wi-Fi»). Over a trusted cable the phone asks nothing.
pub async fn pair_over_cable(udid: &str) -> Result<(), LinkError> {
    let mut mux = UsbmuxdConnection::default()
        .await
        .map_err(|e| LinkError::AppleServiceMissing(format!("{e:?}")))?;
    let device = mux
        .get_devices()
        .await
        .map_err(|e| LinkError::Device(format!("{e:?}")))?
        .into_iter()
        .find(|d| d.udid == udid && matches!(d.connection_type, Connection::Usb))
        .ok_or(LinkError::Device("not on the cable".into()))?;
    let provider = device.to_provider(UsbmuxdAddr::default(), "sirtone");
    let service = RemotePairingLockdownService::connect(&provider)
        .await
        .map_err(|e| LinkError::Apple("phone.wifi_pair", format!("{e:?}")))?;
    let mut client = service
        .into_client(HOST)
        .map_err(|e| LinkError::Apple("phone.wifi_pair", format!("{e:?}")))?;
    let mut rec = RpPairingFile::generate(HOST);
    client
        .connect(&mut rec, async || "000000".to_string())
        .await
        .map_err(|e| LinkError::Apple("phone.wifi_pair", format!("{e:?}")))?;
    save(&keyring(), udid, &rec)
}

/// One of our phones seen on the local network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    pub udid: String,
    /// IPv4 first: link-local IPv6 needs a scope the browse does not give.
    pub addrs: Vec<IpAddr>,
    pub port: u16,
}

/// Browses Bonjour up to `wait` (returns as soon as every paired phone is seen).
pub async fn find(wait: Duration) -> Result<Vec<Sighting>, LinkError> {
    let s = keyring();
    let records: Vec<(String, RpPairingFile)> = ids(&s)
        .into_iter()
        .filter_map(|u| load(&s, &u).map(|r| (u, r)))
        .collect();
    if records.is_empty() {
        return Ok(Vec::new());
    }
    tokio::task::spawn_blocking(move || browse(&records, wait))
        .await
        .map_err(|e| LinkError::Device(e.to_string()))?
}

fn browse(records: &[(String, RpPairingFile)], wait: Duration) -> Result<Vec<Sighting>, LinkError> {
    let mdns = mdns_sd::ServiceDaemon::new().map_err(|e| LinkError::Device(e.to_string()))?;
    let rx = mdns
        .browse(BONJOUR)
        .map_err(|e| LinkError::Device(e.to_string()))?;
    let until = Instant::now() + wait;
    let mut out: Vec<Sighting> = Vec::new();
    while let Some(left) = until.checked_duration_since(Instant::now()) {
        let Ok(ev) = rx.recv_timeout(left) else { break };
        let mdns_sd::ServiceEvent::ServiceResolved(info) = ev else {
            continue;
        };
        let id = info.get_property_val_str("identifier").unwrap_or("");
        let tag = info.get_property_val_str("authTag").unwrap_or("");
        let Some(udid) = recognise(records, id, tag) else {
            continue;
        };
        let mut addrs: Vec<IpAddr> = info.get_addresses().iter().copied().collect();
        addrs.sort_by_key(|a| !a.is_ipv4());
        out.retain(|x| x.udid != udid);
        out.push(Sighting {
            udid: udid.to_string(),
            addrs,
            port: info.get_port(),
        });
        if out.len() == records.len() {
            break;
        }
    }
    let _ = mdns.shutdown();
    Ok(out)
}

async fn connect_any(
    addrs: &[IpAddr],
    port: u16,
) -> Result<(tokio::net::TcpStream, IpAddr), LinkError> {
    for a in addrs {
        let sa = SocketAddr::new(*a, port);
        if let Ok(Ok(s)) =
            tokio::time::timeout(Duration::from_secs(5), tokio::net::TcpStream::connect(sa)).await
        {
            return Ok((s, *a));
        }
    }
    Err(LinkError::Apple("phone.wifi_unreachable", String::new()))
}

/// A live tunnel to the phone: services by RSD, connections through `handle`. Dropping it closes
/// the tunnel (the control connection is kept inside for its lifetime).
pub struct Tunnel {
    pub handle: AdapterHandle,
    pub rsd: RsdHandshake,
    /// The phone's own name from RSD, for registering it with Apple if it is new there.
    pub name: Option<String>,
    _control: RemotePairingClient<RpPairingSocket<tokio::net::TcpStream>>,
}

/// Finds the phone on the network and opens the tunnel (`phone.wifi_not_found` when it is not
/// seen within `wait`: screen off, another network, or Wi-Fi access not set up).
pub async fn open(udid: &str, wait: Duration) -> Result<Tunnel, LinkError> {
    let mut rec =
        load(&keyring(), udid).ok_or(LinkError::Apple("phone.wifi_not_paired", String::new()))?;
    let seen = find(wait)
        .await?
        .into_iter()
        .find(|s| s.udid == udid)
        .ok_or(LinkError::Apple("phone.wifi_not_found", String::new()))?;
    let (stream, ip) = connect_any(&seen.addrs, seen.port).await?;
    let mut control = RemotePairingClient::new(RpPairingSocket::new(stream), HOST);
    let tunnel_err =
        |e: idevice::IdeviceError| LinkError::Apple("phone.wifi_tunnel", format!("{e:?}"));
    control.attempt_pair_verify().await.map_err(tunnel_err)?;
    control
        .validate_pairing(&mut rec)
        .await
        .map_err(|e| LinkError::Apple("phone.wifi_not_paired", format!("{e:?}")))?;
    let port = control.create_tcp_listener().await.map_err(tunnel_err)?;
    let ts = tokio::net::TcpStream::connect(SocketAddr::new(ip, port))
        .await
        .map_err(|_| LinkError::Apple("phone.wifi_unreachable", String::new()))?;
    let cd = connect_tls_psk_tunnel_native(ts, control.encryption_key())
        .await
        .map_err(tunnel_err)?;
    let info = cd.info.clone();
    let ours: IpAddr = info
        .client_address
        .parse()
        .map_err(|_| LinkError::Apple("phone.wifi_tunnel", "address".into()))?;
    let theirs: IpAddr = info
        .server_address
        .parse()
        .map_err(|_| LinkError::Apple("phone.wifi_tunnel", "address".into()))?;
    let mut adapter = Adapter::new(Box::new(cd.into_inner()), ours, theirs);
    adapter.set_mss((info.mtu as usize).saturating_sub(60));
    let mut handle = adapter.to_async_handle();
    let s = handle
        .connect(info.server_rsd_port)
        .await
        .map_err(|e| LinkError::Apple("phone.wifi_tunnel", e.to_string()))?;
    let rsd = Box::pin(RsdHandshake::new(s)).await.map_err(tunnel_err)?;
    let name = rsd
        .properties
        .get("DeviceName")
        .and_then(|v| v.as_string())
        .map(str::to_string);
    Ok(Tunnel {
        handle,
        rsd,
        name,
        _control: control,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use isideload::util::storage::InMemoryStorage;

    fn paired(host: &str, irk: [u8; 16]) -> RpPairingFile {
        let mut r = RpPairingFile::generate(host);
        r.alt_irk = Some(irk.to_vec());
        r
    }

    /// A phone is recognised only by a tag made with its own record's key.
    #[test]
    fn recognises_our_phone_by_auth_tag() {
        use base64::Engine as _;
        let records = vec![
            ("A".to_string(), paired(HOST, [1; 16])),
            ("B".to_string(), paired(HOST, [2; 16])),
        ];
        let tag_b = base64::engine::general_purpose::STANDARD.encode(
            idevice::remote_pairing::compute_auth_tag(&[2; 16], "svc-id"),
        );
        assert_eq!(recognise(&records, "svc-id", &tag_b), Some("B"));
        assert_eq!(recognise(&records, "other-id", &tag_b), None);
        assert_eq!(recognise(&records, "svc-id", "AAAAAAAA"), None);
    }

    /// Records survive a round trip, the index stays in step, forgetting removes one phone only.
    #[test]
    fn records_round_trip_and_forget() {
        let s = InMemoryStorage::new();
        save(&s, "A", &paired(HOST, [1; 16])).unwrap();
        save(&s, "B", &paired(HOST, [2; 16])).unwrap();
        save(&s, "A", &paired(HOST, [3; 16])).unwrap();
        assert_eq!(ids(&s), ["A", "B"]);
        assert_eq!(load(&s, "A").unwrap().alt_irk(), Some(&[3u8; 16][..]));
        remove(&s, "A").unwrap();
        assert!(load(&s, "A").is_none());
        assert_eq!(ids(&s), ["B"]);
    }
}
