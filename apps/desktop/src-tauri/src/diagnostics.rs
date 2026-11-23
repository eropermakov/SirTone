//! Logs and crash reports (T0.5, docs/03 §12).
//!
//! - `logs/sirtone-YYYY-MM-DD.log`, daily rotation, 7 files kept, ≤ 20 MB in total;
//! - panics → `crash/<unix-ms>.txt` (message, location, backtrace, version, OS), then the default hook;
//! - `info` by default, `debug` with `--debug` or `SIRTONE_LOG=debug`.
//!
//! No user paths or track names at info/warn level (docs/12 §3.7).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::State;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

const LOG_PREFIX: &str = "sirtone";
const LOG_SUFFIX: &str = "log";
const KEEP_FILES: usize = 7;
const MAX_TOTAL_BYTES: u64 = 20 * 1024 * 1024;
const DIAGNOSTICS_DIR: &str = "diagnostics";
const INSTALLATION_ID_FILE: &str = "installation-id";
const SESSION_MARKER_FILE: &str = "active-session.json";

/// Identity and lifecycle state for the voluntary diagnostic report. This is deliberately
/// separate from `tracing`: `tracing` remains the operational developer log, while this state
/// has a stable, portable schema. Never call its disk-writing methods from an audio callback.
#[derive(Debug, Clone)]
pub struct DiagnosticSession {
    installation_id: String,
    session_id: String,
    dir: PathBuf,
    previous_unclean_shutdown: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticStatus {
    pub installation_id: String,
    pub session_id: String,
    /// A leftover marker means only that the previous process did not record an orderly exit.
    /// It is intentionally not called a crash.
    pub previous_unclean_shutdown: bool,
}

#[derive(Debug, Serialize)]
struct SessionMarker<'a> {
    schema_version: u8,
    session_id: &'a str,
    started_at_unix_ms: u128,
}

impl DiagnosticSession {
    /// Opens one application session. Failures to persist are non-fatal: playback must never be
    /// blocked by diagnostics, but the returned IDs are still useful for the current process.
    pub fn start(data_dir: &Path) -> Self {
        let dir = data_dir.join(DIAGNOSTICS_DIR);
        let _ = fs::create_dir_all(&dir);
        let id_path = dir.join(INSTALLATION_ID_FILE);
        let installation_id = fs::read_to_string(&id_path)
            .ok()
            .and_then(|text| Uuid::parse_str(text.trim()).ok())
            .map(|id| id.to_string())
            .unwrap_or_else(|| {
                let id = Uuid::now_v7().to_string();
                // A replace, rather than append, also repairs a truncated ID file safely.
                let _ = fs::write(&id_path, format!("{id}\n"));
                id
            });
        let marker = dir.join(SESSION_MARKER_FILE);
        let previous_unclean_shutdown = marker.is_file();
        let session_id = Uuid::now_v7().to_string();
        let value = SessionMarker {
            schema_version: 1,
            session_id: &session_id,
            started_at_unix_ms: unix_ms(),
        };
        if let Ok(bytes) = serde_json::to_vec(&value) {
            // Windows does not replace an existing destination with `rename`. A small marker is
            // deliberately overwritten after its fact has been recorded above; a partial marker
            // is still conservatively treated as an unclean next launch.
            let _ = fs::write(&marker, bytes);
        }
        Self {
            installation_id,
            session_id,
            dir,
            previous_unclean_shutdown,
        }
    }

    pub fn status(&self) -> DiagnosticStatus {
        DiagnosticStatus {
            installation_id: self.installation_id.clone(),
            session_id: self.session_id.clone(),
            previous_unclean_shutdown: self.previous_unclean_shutdown,
        }
    }

    /// Marks only this process's session as clean. A different marker is preserved rather than
    /// hiding another process's diagnostic evidence.
    pub fn finish(&self) {
        let marker = self.dir.join(SESSION_MARKER_FILE);
        let matches_current = fs::read_to_string(&marker)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|v| {
                v.get("session_id")
                    .and_then(|id| id.as_str())
                    .map(str::to_owned)
            })
            .is_some_and(|id| id == self.session_id);
        if matches_current {
            let _ = fs::remove_file(marker);
        }
    }
}

/// Gives the UI only the non-sensitive lifecycle facts needed to offer a voluntary report.
#[tauri::command(async)]
#[specta::specta]
pub fn diagnostics_status(session: State<'_, DiagnosticSession>) -> DiagnosticStatus {
    session.status()
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Application data directory: `%LOCALAPPDATA%\SirTone` (installed mode, docs/03 §8.2);
/// `Data` next to the exe in portable mode (T6.4).
///
/// `SIRTONE_DATA_DIR` overrides it: tests against the real app run on their own settings, logs and
/// library database and never touch the owner's (stage-1 plan).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SIRTONE_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(dir) = crate::portable::data_dir() {
        return dir;
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("SirTone")
}

/// D-132, MAT-16: the Apple and phone libraries stay quiet whatever `SIRTONE_LOG` says — their
/// info lines carry account details (certificate names hold the Apple ID) and idevice's debug
/// lines print pairing handshakes and keys.
const QUIET: [&str; 6] = [
    "isideload=warn",
    "apple_codesign=error",
    "idevice=warn",
    "jktcp=warn",
    "reqwest=warn",
    "hyper=warn",
];

/// `filter` with the [`QUIET`] caps on top (they win over any wider level).
fn quiet(mut filter: EnvFilter) -> EnvFilter {
    for cap in QUIET {
        if let Ok(d) = cap.parse() {
            filter = filter.add_directive(d);
        }
    }
    filter
}

/// Starts file logging. Keep the guard alive for the whole process, otherwise lines are lost.
pub fn init_logging(dir: &Path, debug: bool) -> Option<WorkerGuard> {
    let logs = dir.join("logs");
    if fs::create_dir_all(&logs).is_err() {
        return None;
    }
    enforce_total_cap(&logs, MAX_TOTAL_BYTES);
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_PREFIX)
        .filename_suffix(LOG_SUFFIX)
        .max_log_files(KEEP_FILES)
        .build(&logs)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let default = if debug { "debug" } else { "info" };
    let filter =
        quiet(EnvFilter::try_from_env("SIRTONE_LOG").unwrap_or_else(|_| EnvFilter::new(default)));
    let installed = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .try_init()
        .is_ok();
    installed.then_some(guard)
}

/// Deletes the oldest `*.log` files until the folder fits into `cap` bytes.
pub fn enforce_total_cap(logs: &Path, cap: u64) {
    let Ok(entries) = fs::read_dir(logs) else {
        return;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == LOG_SUFFIX))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    files.sort_by_key(|(t, _, _)| *t);
    let mut total: u64 = files.iter().map(|(_, len, _)| *len).sum();
    for (_, len, path) in files {
        if total <= cap {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

/// Installs a panic hook that writes `crash/<unix-ms>.txt` before the default hook runs.
pub fn install_panic_hook(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".to_owned());
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()));
        let _ = write_crash_report(&dir, &message, location.as_deref());
        tracing::error!(target: "sirtone::crash", %message, location = location.as_deref().unwrap_or("?"), "panic");
        previous(info);
    }));
}

/// Writes one crash report; returns its path.
pub fn write_crash_report(
    dir: &Path,
    message: &str,
    location: Option<&str>,
) -> std::io::Result<PathBuf> {
    let crash = dir.join("crash");
    fs::create_dir_all(&crash)?;
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = crash.join(format!("{ms}.txt"));
    let mut f = fs::File::create(&path)?;
    writeln!(f, "SirTone {} crash report", env!("CARGO_PKG_VERSION"))?;
    writeln!(f, "time_unix_ms: {ms}")?;
    writeln!(f, "os: {} {}", std::env::consts::OS, std::env::consts::ARCH)?;
    writeln!(
        f,
        "thread: {}",
        std::thread::current().name().unwrap_or("<unnamed>")
    )?;
    writeln!(f, "message: {message}")?;
    writeln!(f, "location: {}", location.unwrap_or("?"))?;
    writeln!(
        f,
        "\nbacktrace:\n{}",
        std::backtrace::Backtrace::force_capture()
    )?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MAT-16: even at `SIRTONE_LOG=trace` the Apple and phone libraries write nothing below
    /// their caps — no handshake, key or certificate line reaches the log file.
    #[test]
    fn apple_and_phone_libraries_stay_quiet() {
        use std::sync::{Arc, Mutex};
        #[derive(Clone, Default)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let buf = Buf::default();
        let out = buf.clone();
        let sub = tracing_subscriber::fmt()
            .with_env_filter(quiet(EnvFilter::new("trace")))
            .with_writer(move || out.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(sub, || {
            tracing::debug!(target: "idevice::pairing", "PAIRING-KEY");
            tracing::info!(target: "isideload::dev", "CERT-OWNER-NAME");
            tracing::info!(target: "apple_codesign", "SIGNER-NAME");
            tracing::debug!(target: "jktcp::adapter", "SEGMENT");
            tracing::debug!(target: "sirtone::phones", "our own line");
        });
        let text = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
        assert!(text.contains("our own line"), "{text}");
        for secret in ["PAIRING-KEY", "CERT-OWNER-NAME", "SIGNER-NAME", "SEGMENT"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("sirtone-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn crash_report_contains_message_version_and_location() {
        let dir = tmp("crash");
        let path = write_crash_report(&dir, "boom", Some("src/x.rs:1")).unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("message: boom"));
        assert!(text.contains("location: src/x.rs:1"));
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn panic_in_thread_writes_report_and_does_not_kill_caller() {
        let dir = tmp("hook");
        install_panic_hook(dir.clone());
        let joined = std::thread::spawn(|| panic!("test panic")).join();
        let _ = std::panic::take_hook(); // restore default for other tests
        assert!(joined.is_err());
        let reports = fs::read_dir(dir.join("crash")).unwrap().count();
        assert!(reports >= 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn total_cap_removes_oldest_logs_first() {
        let dir = tmp("cap");
        for (i, size) in [400usize, 400, 400].iter().enumerate() {
            fs::write(
                dir.join(format!("sirtone.2026-10-1{i}.log")),
                vec![b'x'; *size],
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        enforce_total_cap(&dir, 900);
        let left: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left.len(), 2);
        assert!(!left.iter().any(|n| n == "sirtone.2026-10-10.log"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn installation_id_survives_and_session_id_changes() {
        let dir = tmp("identity");
        let one = DiagnosticSession::start(&dir);
        one.finish();
        let two = DiagnosticSession::start(&dir);
        assert_eq!(one.status().installation_id, two.status().installation_id);
        assert_ne!(one.status().session_id, two.status().session_id);
        two.finish();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn leftover_marker_is_not_labeled_as_a_crash() {
        let dir = tmp("unclean");
        let one = DiagnosticSession::start(&dir);
        drop(one); // Simulates a process that did not reach its orderly-exit handler.
        let two = DiagnosticSession::start(&dir);
        assert!(two.status().previous_unclean_shutdown);
        two.finish();
        let _ = fs::remove_dir_all(dir);
    }
}
