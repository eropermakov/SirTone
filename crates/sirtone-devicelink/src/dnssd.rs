//! Bonjour (DNS-SD) browse through Windows' own resolver: `DnsServiceBrowse` (dnsapi.dll,
//! Windows 10 1809+). A userspace mDNS stack (`mdns-sd`) heard nothing on the owner's PC while
//! the system saw the same services (Apple's Bonjour holds UDP 5353, `spike-P-04b.md` §4); the
//! system resolver already has its firewall rules, so no prompt appears either.
//!
//! Each callback carries one discovered instance as a record list (PTR, SRV, TXT, A/AAAA).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows::Win32::NetworkManagement::Dns::{
    DNS_QUERY_REQUEST_VERSION1, DNS_RECORDW, DNS_SERVICE_BROWSE_REQUEST,
    DNS_SERVICE_BROWSE_REQUEST_0, DNS_SERVICE_CANCEL, DNS_TYPE_A, DNS_TYPE_AAAA, DNS_TYPE_SRV,
    DnsFree, DnsFreeRecordList, DnsServiceBrowse, DnsServiceBrowseCancel,
};
use windows::core::PCWSTR;

use crate::LinkError;

const DNS_TYPE_TXT: u16 = 16;
const DNS_REQUEST_PENDING: i32 = 9506;
const ERROR_CANCELLED: u32 = 1223;

/// One service instance seen on the local network.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Service {
    pub instance: String,
    pub port: u16,
    /// IPv4 first: link-local IPv6 needs an interface scope the browse does not give.
    pub addrs: Vec<IpAddr>,
    /// TXT `key=value` pairs.
    pub txt: Vec<(String, String)>,
}

impl Service {
    pub fn txt(&self, key: &str) -> Option<&str> {
        self.txt
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Default)]
struct State {
    found: Vec<Service>,
    cancelled: bool,
}

/// # Safety
/// `p` is null or a NUL-terminated UTF-16 string owned by the record list.
unsafe fn wide(p: windows::core::PWSTR) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { p.to_string() }.unwrap_or_default()
    }
}

/// Turns one callback's record list into a service.
///
/// # Safety
/// `first` is null or the head of a valid `DNS_RECORDW` list from dnsapi.
unsafe fn parse(first: *const DNS_RECORDW) -> Option<Service> {
    let mut s = Service::default();
    let mut rec = first;
    while !rec.is_null() {
        let r = unsafe { &*rec };
        let ty = r.wType;
        if ty == DNS_TYPE_SRV.0 {
            let srv = unsafe { r.Data.SRV };
            s.instance = unsafe { wide(r.pName) };
            s.port = srv.wPort;
        } else if ty == DNS_TYPE_TXT {
            let txt = unsafe { &r.Data.TXT };
            let strings = txt.pStringArray.as_ptr();
            for i in 0..txt.dwStringCount as usize {
                let kv = unsafe { wide(*strings.add(i)) };
                let (k, v) = kv.split_once('=').unwrap_or((kv.as_str(), ""));
                s.txt.push((k.to_string(), v.to_string()));
            }
        } else if ty == DNS_TYPE_A.0 {
            // Network byte order in memory.
            let ip = unsafe { r.Data.A.IpAddress };
            s.addrs.push(IpAddr::V4(Ipv4Addr::from(ip.to_ne_bytes())));
        } else if ty == DNS_TYPE_AAAA.0 {
            let b = unsafe { r.Data.AAAA.Ip6Address.IP6Byte };
            s.addrs.push(IpAddr::V6(Ipv6Addr::from(b)));
        }
        rec = r.pNext;
    }
    s.addrs.sort_by_key(|a| !a.is_ipv4());
    s.addrs.dedup();
    (s.port != 0).then_some(s)
}

unsafe extern "system" fn on_found(
    status: u32,
    ctx: *const core::ffi::c_void,
    records: *const DNS_RECORDW,
) {
    // SAFETY: `ctx` is the `Arc<Mutex<State>>` raw pointer `browse` keeps alive until the
    // cancellation callback (or leaks it if that never comes).
    let state = unsafe { &*(ctx as *const Mutex<State>) };
    if status == ERROR_CANCELLED {
        if let Ok(mut s) = state.lock() {
            s.cancelled = true;
        }
        return;
    }
    if status == 0 && !records.is_null() {
        let svc = unsafe { parse(records) };
        unsafe { DnsFree(Some(records as *const _), DnsFreeRecordList) };
        if let (Some(svc), Ok(mut s)) = (svc, state.lock()) {
            s.found.retain(|x| x.instance != svc.instance);
            s.found.push(svc);
        }
    }
}

/// Browses `service` (e.g. `_remotepairing._tcp.local`) up to `wait`, or until `enough` says
/// what was found suffices. Blocking: call from `spawn_blocking`.
pub fn browse(
    service: &str,
    wait: Duration,
    enough: impl Fn(&[Service]) -> bool,
) -> Result<Vec<Service>, LinkError> {
    let state = Arc::new(Mutex::new(State::default()));
    let ctx = Arc::into_raw(state.clone());
    let name: Vec<u16> = service.encode_utf16().chain([0]).collect();
    let req = DNS_SERVICE_BROWSE_REQUEST {
        Version: DNS_QUERY_REQUEST_VERSION1.0,
        InterfaceIndex: 0,
        QueryName: PCWSTR(name.as_ptr()),
        Anonymous: DNS_SERVICE_BROWSE_REQUEST_0 {
            pBrowseCallback: Some(on_found),
        },
        pQueryContext: ctx as *mut _,
    };
    let mut cancel = DNS_SERVICE_CANCEL::default();
    // SAFETY: `req` and `name` outlive the call; the callback context is kept alive below.
    let rc = unsafe { DnsServiceBrowse(&req, &mut cancel) };
    if rc != DNS_REQUEST_PENDING {
        // SAFETY: no browse started, so no callback holds the pointer.
        drop(unsafe { Arc::from_raw(ctx) });
        return Err(LinkError::Device(format!("dns-sd browse: {rc}")));
    }
    let until = Instant::now() + wait;
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
        if state.lock().is_ok_and(|s| enough(&s.found)) {
            break;
        }
    }
    // SAFETY: the handle came from the successful browse above.
    unsafe { DnsServiceBrowseCancel(&cancel) };
    let drained = Instant::now() + Duration::from_secs(2);
    while Instant::now() < drained {
        if state.lock().is_ok_and(|s| s.cancelled) {
            // SAFETY: dnsapi reported the cancellation: no further callbacks.
            drop(unsafe { Arc::from_raw(ctx) });
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Without the cancellation callback the context stays leaked (a few bytes) rather than
    // risking a late callback on freed memory.
    let found = state.lock().map(|s| s.found.clone()).unwrap_or_default();
    Ok(found)
}
