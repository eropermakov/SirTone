//! Files inside Сиртон's own folder on the iPhone (M6.2, FR-XFER; `docs/mobile/README.md` §11):
//! `house_arrest` vends the app's container over AFC — over the cable, or through the Wi-Fi tunnel
//! ([`crate::wifi`]). The app need not be running; it picks packages up when opened.
//!
//! Layout the app and the hub agree on:
//! - [`INBOX`]`/<package>/` — a package from the PC (`sirtone-transfer` folder); the hub writes
//!   [`COMPLETE`] last, so the app never applies a half-copied package.
//! - [`LIBRARY`] — the phone's library root (`.sirtone/library.db` + music), read for «Забрать с
//!   телефона».
//!
//! Copies resume: a file already there with the same size is skipped (the package manifest
//! names every file and its size, `sirtone-transfer`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use idevice::afc::AfcClient;
use idevice::afc::opcode::AfcFopenMode;
use idevice::house_arrest::HouseArrestClient;
use idevice::installation_proxy::InstallationProxyClient;
use idevice::usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdConnection};
use idevice::{IdeviceError, IdeviceService, RsdService};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::LinkError;

/// The app's bundle identifier before the team suffix (D-136).
pub const BUNDLE_PREFIX: &str = "studio.serkov.sirtone.mobile";
/// Packages from the PC.
pub const INBOX: &str = "/Documents/Transfer/Inbox";
/// Written last into a package folder: the package is whole.
pub const COMPLETE: &str = ".complete";
/// The phone's library root.
pub const LIBRARY: &str = "/Library/Application Support/SirTone";

const CHUNK: usize = 1 << 20;

fn afc_err(e: IdeviceError) -> LinkError {
    LinkError::Apple("phone.transfer", format!("{e:?}"))
}

/// The app's container on one phone; keeps the Wi-Fi tunnel (if any) alive while in use.
pub struct AppFiles {
    afc: AfcClient,
    /// The installed app's identifier (`studio.serkov.sirtone.mobile.<TEAM>`).
    pub bundle_id: String,
    _tunnel: Option<crate::wifi::Tunnel>,
}

fn ours(apps: impl IntoIterator<Item = String>) -> Option<String> {
    apps.into_iter()
        .filter(|id| id.starts_with(&format!("{BUNDLE_PREFIX}.")))
        .min()
}

/// Opens the app's container: the cable when the phone is on it, else Wi-Fi.
/// `phone.app_not_installed` — Сиртон is not on this phone.
pub async fn open(udid: &str) -> Result<AppFiles, LinkError> {
    let cable = match UsbmuxdConnection::default().await {
        Ok(mut mux) => mux.get_devices().await.ok().and_then(|ds| {
            ds.into_iter()
                .find(|d| d.udid == udid && matches!(d.connection_type, Connection::Usb))
        }),
        Err(_) => None,
    };
    let not_installed = || LinkError::Apple("phone.app_not_installed", String::new());
    match cable {
        Some(device) => {
            let provider = device.to_provider(UsbmuxdAddr::default(), "sirtone");
            let mut ip = InstallationProxyClient::connect(&provider)
                .await
                .map_err(afc_err)?;
            let apps = ip.get_apps(Some("User"), None).await.map_err(afc_err)?;
            let bundle_id = ours(apps.into_keys()).ok_or_else(not_installed)?;
            let ha = HouseArrestClient::connect(&provider)
                .await
                .map_err(afc_err)?;
            let afc = ha.vend_container(&bundle_id).await.map_err(afc_err)?;
            Ok(AppFiles {
                afc,
                bundle_id,
                _tunnel: None,
            })
        }
        None if crate::wifi::is_paired(udid) => {
            let mut t = Box::pin(crate::wifi::open(udid, Duration::from_secs(10))).await?;
            let mut ip = InstallationProxyClient::connect_rsd(&mut t.handle, &mut t.rsd)
                .await
                .map_err(afc_err)?;
            let apps = ip.get_apps(Some("User"), None).await.map_err(afc_err)?;
            let bundle_id = ours(apps.into_keys()).ok_or_else(not_installed)?;
            let ha = HouseArrestClient::connect_rsd(&mut t.handle, &mut t.rsd)
                .await
                .map_err(afc_err)?;
            let afc = ha.vend_container(&bundle_id).await.map_err(afc_err)?;
            Ok(AppFiles {
                afc,
                bundle_id,
                _tunnel: Some(t),
            })
        }
        None => Err(LinkError::Device("not connected".into())),
    }
}

/// What a copy did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Copied {
    pub files: u32,
    /// Already there with the same size (a resumed copy).
    pub skipped: u32,
    pub bytes: u64,
    /// Sources that could not be read (moved away since the export); the rest went on.
    pub missing: u32,
}

/// Every file under `dir`, as (relative path with `/`, size).
fn local_files(dir: &Path) -> std::io::Result<Vec<(String, u64)>> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, u64)>) -> std::io::Result<()> {
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let p = e.path();
            if e.file_type()?.is_dir() {
                walk(base, &p, out)?;
            } else {
                let rel = p
                    .strip_prefix(base)
                    .unwrap_or(&p)
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push((rel, e.metadata()?.len()));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out)?;
    out.sort();
    Ok(out)
}

/// A package folder name on the phone: the package id without path-hostile characters.
pub fn package_folder(package_id: &str) -> String {
    package_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn parent(path: &str) -> Option<&str> {
    path.rfind('/')
        .map(|i| &path[..i])
        .filter(|p| !p.is_empty())
}

impl AppFiles {
    async fn size(&mut self, path: &str) -> Option<u64> {
        self.afc
            .get_file_info(path)
            .await
            .ok()
            .filter(|i| i.st_ifmt == "S_IFREG")
            .map(|i| i.size as u64)
    }

    async fn mkdirs(&mut self, dir: &str) -> Result<(), LinkError> {
        let mut at = String::new();
        for part in dir.split('/').filter(|p| !p.is_empty()) {
            at.push('/');
            at.push_str(part);
            // Exists already → AFC answers with an error we can ignore; a real failure shows on
            // the file write that follows.
            let _ = self.afc.mk_dir(at.clone()).await;
        }
        Ok(())
    }

    /// Writes small bytes as one file (markers).
    pub async fn write(&mut self, path: &str, bytes: &[u8]) -> Result<(), LinkError> {
        if let Some(dir) = parent(path) {
            self.mkdirs(dir).await?;
        }
        let mut f = self
            .afc
            .open(path, AfcFopenMode::WrOnly)
            .await
            .map_err(afc_err)?;
        f.write_entire(bytes).await.map_err(afc_err)?;
        f.close().await.map_err(afc_err)
    }

    /// Copies the folder `local` to `remote` on the phone, resuming by size. `progress` gets
    /// (bytes done, bytes total).
    pub async fn push_dir(
        &mut self,
        local: &Path,
        remote: &str,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<Copied, LinkError> {
        let files = local_files(local)
            .map_err(|e| LinkError::Apple("phone.transfer", e.to_string()))?
            .into_iter()
            .map(|(rel, _)| {
                (
                    rel.split('/').fold(local.to_path_buf(), |p, s| p.join(s)),
                    rel,
                )
            })
            .collect::<Vec<_>>();
        self.push_files(&files, remote, progress).await
    }

    /// Copies `(local file, path under remote)` pairs to the phone, resuming by size; a source
    /// that cannot be read is skipped (counted in `missing`), the rest goes on.
    pub async fn push_files(
        &mut self,
        files: &[(PathBuf, String)],
        remote: &str,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<Copied, LinkError> {
        let sized: Vec<(&PathBuf, &String, u64)> = files
            .iter()
            .filter_map(|(l, r)| std::fs::metadata(l).ok().map(|m| (l, r, m.len())))
            .collect();
        let total: u64 = sized.iter().map(|(_, _, n)| n).sum();
        let mut done = 0u64;
        let mut c = Copied {
            missing: (files.len() - sized.len()) as u32,
            ..Copied::default()
        };
        let mut made = std::collections::HashSet::new();
        let io = |e: std::io::Error| LinkError::Apple("phone.transfer", e.to_string());
        for (local, rel, len) in sized {
            let path = format!("{remote}/{rel}");
            if self.size(&path).await == Some(len) {
                c.skipped += 1;
                done += len;
                progress(done, total);
                continue;
            }
            if let Some(dir) = parent(&path)
                && made.insert(dir.to_string())
            {
                self.mkdirs(dir).await?;
            }
            let Ok(mut src) = tokio::fs::File::open(local).await else {
                c.missing += 1;
                continue;
            };
            let mut f = self
                .afc
                .open(path, AfcFopenMode::WrOnly)
                .await
                .map_err(afc_err)?;
            let mut buf = vec![0u8; CHUNK];
            loop {
                let n = src.read(&mut buf).await.map_err(io)?;
                if n == 0 {
                    break;
                }
                f.write_all(&buf[..n]).await.map_err(io)?;
                done += n as u64;
                c.bytes += n as u64;
                progress(done, total);
            }
            f.close().await.map_err(afc_err)?;
            c.files += 1;
        }
        Ok(c)
    }

    /// Every file under `remote` (relative paths with `/`, sizes).
    pub async fn list_files(&mut self, remote: &str) -> Result<Vec<(String, u64)>, LinkError> {
        let mut out = Vec::new();
        let mut dirs = vec![String::new()];
        while let Some(rel) = dirs.pop() {
            let at = if rel.is_empty() {
                remote.to_string()
            } else {
                format!("{remote}/{rel}")
            };
            let Ok(names) = self.afc.list_dir(at.clone()).await else {
                continue;
            };
            for name in names.into_iter().filter(|n| n != "." && n != "..") {
                let child_rel = if rel.is_empty() {
                    name.clone()
                } else {
                    format!("{rel}/{name}")
                };
                let Ok(info) = self.afc.get_file_info(format!("{at}/{name}")).await else {
                    continue;
                };
                match info.st_ifmt.as_str() {
                    "S_IFDIR" => dirs.push(child_rel),
                    "S_IFREG" => out.push((child_rel, info.size as u64)),
                    _ => {}
                }
            }
        }
        out.sort();
        Ok(out)
    }

    /// Copies files under `remote` that `want` accepts into `local`, resuming by size.
    pub async fn pull_dir(
        &mut self,
        remote: &str,
        local: &Path,
        want: &(dyn Fn(&str) -> bool + Send + Sync),
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<Copied, LinkError> {
        let files: Vec<(String, u64)> = self
            .list_files(remote)
            .await?
            .into_iter()
            .filter(|(rel, _)| want(rel))
            .collect();
        let total: u64 = files.iter().map(|(_, n)| n).sum();
        let io = |e: std::io::Error| LinkError::Apple("phone.transfer", e.to_string());
        let mut done = 0u64;
        let mut c = Copied::default();
        for (rel, len) in files {
            let dst: PathBuf = rel.split('/').fold(local.to_path_buf(), |p, s| p.join(s));
            if std::fs::metadata(&dst).is_ok_and(|m| m.len() == len) {
                c.skipped += 1;
                done += len;
                progress(done, total);
                continue;
            }
            if let Some(d) = dst.parent() {
                std::fs::create_dir_all(d).map_err(io)?;
            }
            let part = dst.with_extension("part");
            let mut out = tokio::fs::File::create(&part).await.map_err(io)?;
            let mut f = self
                .afc
                .open(format!("{remote}/{rel}"), AfcFopenMode::RdOnly)
                .await
                .map_err(afc_err)?;
            let mut buf = vec![0u8; CHUNK];
            loop {
                let n = f.read(&mut buf).await.map_err(io)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n]).await.map_err(io)?;
                done += n as u64;
                c.bytes += n as u64;
                progress(done, total);
            }
            f.close().await.map_err(afc_err)?;
            out.flush().await.map_err(io)?;
            drop(out);
            std::fs::rename(&part, &dst).map_err(io)?;
            c.files += 1;
        }
        Ok(c)
    }

    /// Sends a package folder to the phone's inbox and marks it whole. `extra` — files kept
    /// elsewhere (the music, straight from the library) with their paths inside the package.
    pub async fn send_package(
        &mut self,
        package_dir: &Path,
        package_id: &str,
        extra: &[(PathBuf, String)],
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<Copied, LinkError> {
        let remote = format!("{INBOX}/{}", package_folder(package_id));
        let mut files = local_files(package_dir)
            .map_err(|e| LinkError::Apple("phone.transfer", e.to_string()))?
            .into_iter()
            .map(|(rel, _)| {
                (
                    rel.split('/')
                        .fold(package_dir.to_path_buf(), |p, s| p.join(s)),
                    rel,
                )
            })
            .collect::<Vec<_>>();
        files.extend_from_slice(extra);
        let c = self.push_files(&files, &remote, progress).await?;
        self.write(&format!("{remote}/{COMPLETE}"), b"1").await?;
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_our_app_among_others() {
        let apps = [
            "com.other.app",
            "studio.serkov.sirtone.mobile.ABCDE12345",
            "studio.serkov.sirtone.mobilex.Z",
        ];
        assert_eq!(
            ours(apps.iter().map(|s| s.to_string())).as_deref(),
            Some("studio.serkov.sirtone.mobile.ABCDE12345")
        );
        assert_eq!(ours(["com.other.app".to_string()]), None);
    }

    #[test]
    fn package_folders_are_path_safe() {
        assert_eq!(package_folder("a1b2:1730000000000"), "a1b2-1730000000000");
        assert_eq!(package_folder("../x/y"), "..-x-y");
        assert_eq!(
            parent("/Documents/Transfer/Inbox/p/Music/a.flac"),
            Some("/Documents/Transfer/Inbox/p/Music")
        );
        assert_eq!(parent("/x"), None);
    }

    #[test]
    fn lists_local_files_with_sizes() {
        let dir = std::env::temp_dir().join(format!("sirtone-appfiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Music/A")).unwrap();
        std::fs::write(dir.join("manifest.json"), b"{}").unwrap();
        std::fs::write(dir.join("Music/A/t.flac"), b"12345").unwrap();
        let files = local_files(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            files,
            vec![
                ("Music/A/t.flac".to_string(), 5),
                ("manifest.json".to_string(), 2)
            ]
        );
    }
}
