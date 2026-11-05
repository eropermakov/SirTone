//! The signing interface `isideload` 0.5.2 calls (`ProvisioningProfile`, `RustCryptoCmsSigner`,
//! `BundleSigningSettings`, `sign_bundle`), implemented on `apple-codesign` (rcodesign, MPL-2.0)
//! instead of the LGPL `apple-codesign-quick` (D-143). Written from the interface only.
//!
//! Signing is offline: no time-stamp server (apple-codesign sets none unless asked).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use apple_codesign::cryptography::InMemoryPrivateKey;
use apple_codesign::{BundleSigner, SettingsScope, SigningSettings};
use cryptographic_message_syntax::SignedData;
use plist::{Dictionary, Value};
use rsa::RsaPrivateKey;
use rsa::pkcs8::EncodePrivateKey;
use x509_cert::Certificate;
use x509_cert::der::Encode;
use x509_certificate::CapturedX509Certificate;

#[derive(Debug, thiserror::Error)]
pub enum CodeSignError {
    #[error("provisioning profile: {0}")]
    ProvisioningProfile(String),
    #[error("certificate: {0}")]
    Certificate(String),
    #[error("bundle: {0}")]
    Bundle(String),
    #[error("signing: {0}")]
    Signing(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CodeSignError>;

/// A `.mobileprovision`: CMS SignedData around an XML plist.
#[derive(Debug, Clone)]
pub struct ProvisioningProfile {
    plist: Dictionary,
    team_id: String,
    entitlements: Dictionary,
    certificate_chain: Vec<Vec<u8>>,
}

impl ProvisioningProfile {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let pp = |m: &str| CodeSignError::ProvisioningProfile(m.into());
        let signed = SignedData::parse_ber(data).map_err(|e| pp(&e.to_string()))?;
        let content = signed.signed_content().ok_or_else(|| pp("no embedded plist"))?;
        let plist = match Value::from_reader_xml(content).map_err(|e| pp(&e.to_string()))? {
            Value::Dictionary(d) => d,
            _ => return Err(pp("plist is not a dictionary")),
        };
        let team_id = plist
            .get("TeamIdentifier")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_string)
            .ok_or_else(|| pp("no TeamIdentifier"))?
            .to_string();
        let entitlements = plist
            .get("Entitlements")
            .and_then(Value::as_dictionary)
            .cloned()
            .ok_or_else(|| pp("no Entitlements"))?;
        // The certificates that sign the profile (Apple's chain): isideload picks the
        // intermediates for the developer certificate from them.
        // Their original bytes (DER as Apple wrote them): re-encoding a BER-parsed value panics in
        // bcder.
        let certificate_chain = signed
            .certificates()
            .map(|c| c.constructed_data().to_vec())
            .collect::<Vec<_>>();
        Ok(Self {
            plist,
            team_id,
            entitlements,
            certificate_chain,
        })
    }

    pub fn plist(&self) -> &Dictionary {
        &self.plist
    }

    pub fn team_id(&self) -> &str {
        &self.team_id
    }

    pub fn entitlements(&self) -> &Dictionary {
        &self.entitlements
    }

    pub fn certificate_chain_der(&self) -> &[Vec<u8>] {
        &self.certificate_chain
    }
}

/// What signs: the developer key, its certificate and the chain up to Apple's root.
pub trait CmsSigner: Sync {
    fn key(&self) -> &RsaPrivateKey;
    fn certificate(&self) -> &Certificate;
    fn chain(&self) -> &[Certificate];
}

pub struct RustCryptoCmsSigner {
    key: RsaPrivateKey,
    certificate: Certificate,
    chain: Vec<Certificate>,
}

impl RustCryptoCmsSigner {
    pub fn new(private_key: RsaPrivateKey, certificate: Certificate, chain: Vec<Certificate>) -> Self {
        Self {
            key: private_key,
            certificate,
            chain,
        }
    }
}

impl CmsSigner for RustCryptoCmsSigner {
    fn key(&self) -> &RsaPrivateKey {
        &self.key
    }
    fn certificate(&self) -> &Certificate {
        &self.certificate
    }
    fn chain(&self) -> &[Certificate] {
        &self.chain
    }
}

pub struct BundleSigningSettings<'a> {
    pub team_id: String,
    pub main_entitlements: Dictionary,
    pub entitlements_by_bundle_id: BTreeMap<String, Dictionary>,
    pub cms_signer: Option<&'a dyn CmsSigner>,
    pub embedded_mobileprovision: Option<&'a [u8]>,
    pub embedded_mobileprovisions_by_bundle_id: BTreeMap<String, &'a [u8]>,
    pub cms_blob_reservation: usize,
}

impl<'a> BundleSigningSettings<'a> {
    pub fn new(
        team_id: impl Into<String>,
        main_entitlements: Dictionary,
        cms_signer: Option<&'a dyn CmsSigner>,
    ) -> Self {
        Self {
            team_id: team_id.into(),
            main_entitlements,
            entitlements_by_bundle_id: BTreeMap::new(),
            cms_signer,
            embedded_mobileprovision: None,
            embedded_mobileprovisions_by_bundle_id: BTreeMap::new(),
            cms_blob_reservation: 16 * 1024,
        }
    }
}

fn captured(cert: &Certificate) -> Result<CapturedX509Certificate> {
    let der = cert
        .to_der()
        .map_err(|e| CodeSignError::Certificate(e.to_string()))?;
    CapturedX509Certificate::from_der(der).map_err(|e| CodeSignError::Certificate(e.to_string()))
}

fn bundle_id(dir: &Path) -> Option<String> {
    let v = Value::from_file(dir.join("Info.plist")).ok()?;
    v.as_dictionary()?
        .get("CFBundleIdentifier")?
        .as_string()
        .map(str::to_string)
}

/// Nested bundles (`PlugIns/*.appex`, `Extensions/*.appex`), relative to the app.
fn nested_bundles(app: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for sub in ["PlugIns", "Extensions"] {
        let Ok(rd) = std::fs::read_dir(app.join(sub)) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "appex") {
                let rel = format!("{sub}/{}", e.file_name().to_string_lossy());
                out.push((rel, p));
            }
        }
    }
    out
}

fn entitlements_xml(d: &Dictionary) -> Result<String> {
    let mut buf = Vec::new();
    Value::Dictionary(d.clone())
        .to_writer_xml(&mut buf)
        .map_err(|e| CodeSignError::Signing(e.to_string()))?;
    String::from_utf8(buf).map_err(|e| CodeSignError::Signing(e.to_string()))
}

/// Signs the bundle in place: profile files first (they are sealed in the signature), nested
/// bundles get their own entitlements, then apple-codesign signs leaf bundles before the app.
pub fn sign_bundle(path: impl Into<PathBuf>, settings: &BundleSigningSettings<'_>) -> Result<()> {
    let app: PathBuf = path.into();
    let signer = settings
        .cms_signer
        .ok_or_else(|| CodeSignError::Signing("no signer".into()))?;

    if let Some(p) = settings.embedded_mobileprovision {
        std::fs::write(app.join("embedded.mobileprovision"), p)?;
    }
    let nested = nested_bundles(&app);
    for (_, dir) in &nested {
        if let Some(p) = bundle_id(dir).and_then(|id| settings.embedded_mobileprovisions_by_bundle_id.get(&id)) {
            std::fs::write(dir.join("embedded.mobileprovision"), p)?;
        }
    }

    let key_der = signer
        .key()
        .to_pkcs8_der()
        .map_err(|e| CodeSignError::Certificate(e.to_string()))?;
    let key = InMemoryPrivateKey::from_pkcs8_der(key_der.as_bytes())
        .map_err(|e| CodeSignError::Certificate(e.to_string()))?;

    let mut s = SigningSettings::default();
    s.set_signing_key(&key, captured(signer.certificate())?);
    for c in signer.chain() {
        s.chain_certificate(captured(c)?);
    }
    s.set_team_id(&settings.team_id);
    s.set_entitlements_xml(SettingsScope::Main, entitlements_xml(&settings.main_entitlements)?)
        .map_err(|e| CodeSignError::Signing(e.to_string()))?;
    for (rel, dir) in &nested {
        if let Some(ent) = bundle_id(dir).and_then(|id| settings.entitlements_by_bundle_id.get(&id)) {
            s.set_entitlements_xml(SettingsScope::Path(rel.clone()), entitlements_xml(ent)?)
                .map_err(|e| CodeSignError::Signing(e.to_string()))?;
        }
    }

    let bundle = BundleSigner::new_from_path(&app).map_err(|e| CodeSignError::Bundle(e.to_string()))?;
    bundle
        .write_signed_bundle(&app, &s)
        .map_err(|e| CodeSignError::Signing(e.to_string()))?;
    Ok(())
}
