//! The iPhone app for the hub's install (D-154, FR-HUB-006): an unsigned `.ipa` from the update
//! channel in GitHub Releases, taken only after its sha256 and the channel's signature (the key
//! that signs the PC updates) check out; the hub then signs it with the phone owner's Apple ID.
//! Kept by content in the cache folder, so a second phone needs no second download.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use minisign_verify::{PublicKey, Signature};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_net::{Host, Net, NetError};

const MANIFEST: &str = "update-channel/ios-latest.json";

/// `ios-latest.json`, written by `scripts/release-ios.ps1`.
#[derive(Debug, Deserialize)]
struct Manifest {
    /// Under the channel's base, e.g. `mobile-v0.1.42/sirtone-ios-0.1.42-unsigned.ipa`.
    file: String,
    sha256: String,
    /// The minisign signature of the file, as `tauri signer sign` writes it.
    signature: String,
}

fn text(b64: &str) -> Option<String> {
    String::from_utf8(STANDARD.decode(b64.trim()).ok()?).ok()
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The bytes are the ones the channel published: the sha256 matches and the signature is valid.
fn genuine(data: &[u8], sha256: &str, signature: &str, pubkey: &str) -> bool {
    if !sha256_hex(data).eq_ignore_ascii_case(sha256) {
        return false;
    }
    let (Some(key), Some(sig)) = (text(pubkey), text(signature)) else {
        return false;
    };
    let (Ok(key), Ok(sig)) = (PublicKey::decode(&key), Signature::decode(&sig)) else {
        return false;
    };
    key.verify(data, &sig, true).is_ok()
}

fn missing(details: &str) -> AppError {
    AppError::new(ErrorCode::PhonePackageMissing).with_details(details.to_owned())
}

fn net_error(e: NetError) -> AppError {
    match e {
        NetError::Status(_) | NetError::TooLarge => missing("ios_package.channel"),
        other => other.into(),
    }
}

/// The current package in `dir`: from the cache when it is still the published one, else
/// downloaded. Blocking (the network layer is).
pub fn fetch(net: &Net, dir: &Path, pubkey: &str) -> Result<PathBuf, AppError> {
    let body = net.get(Host::Releases, MANIFEST).map_err(net_error)?;
    let m: Manifest = serde_json::from_slice(&body).map_err(|_| missing("ios_package.manifest"))?;
    if m.sha256.len() != 64 || !m.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(missing("ios_package.manifest"));
    }
    let path = dir.join(format!("{}.ipa", m.sha256.to_ascii_lowercase()));
    if std::fs::read(&path).is_ok_and(|d| genuine(&d, &m.sha256, &m.signature, pubkey)) {
        return Ok(path);
    }
    let data = net.get(Host::Releases, &m.file).map_err(net_error)?;
    if !genuine(&data, &m.sha256, &m.signature, pubkey) {
        tracing::warn!(target: "sirtone::phones", "ios package failed verification");
        return Err(missing("ios_package.signature"));
    }
    let io = |_| AppError::new(ErrorCode::Internal).with_details("ios_package.io");
    // Older packages go: only the published one is ever installed.
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(io)?;
    let part = path.with_extension("part");
    std::fs::write(&part, &data).map_err(io)?;
    std::fs::rename(&part, &path).map_err(io)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use sirtone_net::{Response, Transport};

    use super::*;

    // A throwaway key pair (not the channel's), and `tauri signer sign` of DATA with it.
    const DATA: &[u8] = b"sirtone ios package";
    const SHA: &str = "fade835b137cc82eac6bf194645cb5a28687bfe161b93f658fcb13c4c2db74e2";
    const PUB: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEM3NENFNzA4MEY2NEQ3OEQKUldTTjEyUVBDT2RNeDZTeVoxQWpOQVRqcEpZRktaM1ExaHNxZ2xIaklURDRrNmwzWFh1OEVndHEK";
    const SIG: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTTjEyUVBDT2RNeDBpK3BaczhUZUl4M2NoUUtVaUtnVGpPL09LVW1zRWJkdzE1TmI5NjIwZHYzdXh2VVc2clc0S0IzcTArZGw4YzcxSVZYZ0RuL2p1WUR5ekZmUzR5U2drPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk0MTUzMTExCWZpbGU6ZGF0YS5iaW4KYzF3aGtJMTAwcFpjSlhnbzBFb3Y5TDlDak90bWNPRGg2ZmhmMDZCMkJ4NDUwcmZrS3BPdjN4Q3BwNEdJR21zelBaU2V3QlR2UnoyTHUxcUNPZmtmQmc9PQo=";

    struct Channel {
        file: Vec<u8>,
        asked: std::sync::Arc<Mutex<Vec<String>>>,
    }

    impl Transport for Channel {
        fn get(&self, url: &str, _ua: &str, _max: usize) -> Result<Response, NetError> {
            self.asked.lock().unwrap().push(url.to_owned());
            let body = if url.ends_with("ios-latest.json") {
                format!(
                    r#"{{"version":"1","file":"m/a.ipa","sha256":"{SHA}","signature":"{SIG}"}}"#
                )
                .into_bytes()
            } else {
                self.file.clone()
            };
            Ok(Response { status: 200, body })
        }
    }

    fn net(file: &[u8]) -> (Net, std::sync::Arc<Mutex<Vec<String>>>) {
        let asked = std::sync::Arc::new(Mutex::new(Vec::new()));
        let t = Channel {
            file: file.to_vec(),
            asked: asked.clone(),
        };
        (Net::new(Box::new(t), "test".into()), asked)
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sirtone-ios-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn only_the_signed_bytes_pass() {
        assert!(genuine(DATA, SHA, SIG, PUB));
        assert!(!genuine(b"other bytes", SHA, SIG, PUB), "sha256");
        let other = sha256_hex(b"other bytes");
        assert!(!genuine(b"other bytes", &other, SIG, PUB), "signature");
        assert!(!genuine(DATA, SHA, "not a signature", PUB));
    }

    #[test]
    fn downloads_once_and_keeps_it() {
        let d = dir("once");
        let (n, asked) = net(DATA);
        let p = fetch(&n, &d, PUB).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), DATA);
        assert_eq!(asked.lock().unwrap().len(), 2);
        assert_eq!(fetch(&n, &d, PUB).unwrap(), p);
        assert_eq!(asked.lock().unwrap().len(), 3, "only the manifest again");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_tampered_package_is_not_kept() {
        let d = dir("bad");
        let (n, _) = net(b"tampered");
        let e = fetch(&n, &d, PUB).unwrap_err();
        assert_eq!(e.code, ErrorCode::PhonePackageMissing);
        assert!(!d.exists(), "nothing written");
    }
}
