//! Logs and crash reports (T0.5, docs/03 §12).
//!
//! - `logs/sirtone-YYYY-MM-DD.log`, daily rotation, 7 files kept, ≤ 20 MB in total;
//! - panics → `crash/<unix-ms>.txt` (message, location, backtrace, version, OS), then the default hook;
//! - `info` by default, `debug` with `--debug` or `SIRTONE_LOG=debug`.
//!
//! No user paths or track names at info/warn level (docs/12 §3.7).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
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
const EVENT_PREFIX: &str = "events-";
const EVENT_SUFFIX: &str = "jsonl";
const EVENT_KEEP_DAYS: u64 = 7;
const EVENT_MAX_TOTAL_BYTES: u64 = 10 * 1024 * 1024;
const REPORT_MAX_BYTES: u64 = 5 * 1024 * 1024;
const REPORT_EVENT_BYTES: usize = 4 * 1024 * 1024;
const CONTEXT_ALLOWLIST: &[&str] = &[
    "attempt",
    "audio_device_kind",
    "error_kind",
    "network_state",
    "operation_state",
    "retry_after_ms",
];

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

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReportExport {
    pub report_id: String,
    pub path: String,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
struct SessionMarker<'a> {
    schema_version: u8,
    session_id: &'a str,
    started_at_unix_ms: u128,
}

/// A portable, intentionally small event. `technical_context` is created only through the
/// allow-list below; arbitrary error strings, paths and account details cannot enter it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiagnosticEvent {
    schema_version: u8,
    timestamp_utc: String,
    level: String,
    event_id: String,
    session_id: String,
    platform: String,
    app_version: String,
    build_id: String,
    release_channel: String,
    component: String,
    operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
    message: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    technical_context: BTreeMap<String, String>,
    recoverable: bool,
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

    /// Appends one safe JSONL event. This is for control/UI/background work only, never an audio
    /// real-time callback: it may allocate, sort a small directory and write a file.
    pub fn record(
        &self,
        level: &str,
        event_id: &str,
        component: &str,
        operation: &str,
        error_code: Option<&str>,
        technical_context: &BTreeMap<String, String>,
        recoverable: bool,
    ) {
        let event = DiagnosticEvent {
            schema_version: 1,
            timestamp_utc: iso8601_utc(unix_ms()),
            level: safe_atom(level),
            event_id: safe_atom(event_id),
            session_id: self.session_id.clone(),
            platform: "windows".to_owned(),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            build_id: env!("CARGO_PKG_VERSION").to_owned(),
            release_channel: "development".to_owned(),
            component: safe_atom(component),
            operation: safe_atom(operation),
            error_code: error_code.map(safe_atom),
            message: safe_atom(event_id),
            technical_context: allowed_context(technical_context),
            recoverable,
        };
        let Ok(line) = serde_json::to_vec(&event) else {
            return;
        };
        let file = self
            .dir
            .join(format!("{EVENT_PREFIX}{}.{}", utc_date(), EVENT_SUFFIX));
        if let Ok(mut output) = fs::OpenOptions::new().create(true).append(true).open(file) {
            let _ = output
                .write_all(&line)
                .and_then(|_| output.write_all(b"\n"));
        }
        enforce_event_retention(&self.dir, EVENT_KEEP_DAYS, EVENT_MAX_TOTAL_BYTES);
    }

    /// Exports a portable diagnostic ZIP after an explicit user action. Events are parsed and
    /// serialised again, which drops unknown fields from a damaged or manually edited JSONL file.
    pub fn export_report(&self, destination: &Path) -> std::io::Result<ReportExport> {
        fs::create_dir_all(destination)?;
        let report_id = Uuid::now_v7().to_string();
        let events = recent_events(&self.dir, std::time::Duration::from_secs(86_400));
        let (events, truncated) = cap_events(events, REPORT_EVENT_BYTES);
        let event_lines = events
            .iter()
            .filter_map(|event| serde_json::to_string(event).ok())
            .collect::<Vec<_>>()
            .join("\n");
        let errors: Vec<&DiagnosticEvent> = events
            .iter()
            .filter(|event| event.level == "ERROR")
            .collect();
        let primary = errors
            .first()
            .map_or("none", |event| event.event_id.as_str());
        let summary = format!(
            "SIRTONE Diagnostic Report\nReport ID: {report_id}\nPlatform: Windows\nVersion: {}\nBuild ID: {}\nOperating system: {} {}\nGenerated: {}\nPrimary error: {primary}\nRelated errors: {}\nCrash event: no native crash evidence in this export\nDiagnostics truncated: {}\n",
            env!("CARGO_PKG_VERSION"),
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            iso8601_utc(unix_ms()),
            errors.len(),
            if truncated { "yes" } else { "no" },
        );
        let manifest = serde_json::json!({
            "schema_version": 1,
            "report_id": report_id,
            "installation_id": self.installation_id,
            "session_id": self.session_id,
            "platform": "windows",
            "app_version": env!("CARGO_PKG_VERSION"),
            "build_id": env!("CARGO_PKG_VERSION"),
            "release_channel": "development",
            "truncated": truncated,
        });
        let environment = serde_json::json!({
            "os": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
        });
        let readme = "SIRTONE diagnostic report format 1.\nThis archive contains no music, library database, track names, file paths, listening history, account data, passwords or tokens.\n";
        let path = destination.join(format!("sirtone-report-{report_id}.zip"));
        write_zip(
            &path,
            [
                ("summary.txt", summary.into_bytes()),
                (
                    "manifest.json",
                    serde_json::to_vec_pretty(&manifest).map_err(json_io)?,
                ),
                (
                    "environment.json",
                    serde_json::to_vec_pretty(&environment).map_err(json_io)?,
                ),
                ("events.jsonl", event_lines.into_bytes()),
                (
                    "errors.json",
                    serde_json::to_vec_pretty(&errors).map_err(json_io)?,
                ),
                ("README.txt", readme.as_bytes().to_vec()),
            ],
        )?;
        if fs::metadata(&path)?.len() > REPORT_MAX_BYTES {
            let _ = fs::remove_file(&path);
            return Err(std::io::Error::other(
                "diagnostic report exceeds its size limit",
            ));
        }
        Ok(ReportExport {
            report_id,
            path: path.to_string_lossy().into_owned(),
            truncated,
        })
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

fn safe_atom(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
        .take(96)
        .collect()
}

fn allowed_context(raw: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    raw.iter()
        .filter(|(key, _)| CONTEXT_ALLOWLIST.contains(&key.as_str()))
        .filter_map(|(key, value)| {
            let cleaned = safe_atom(value);
            (!cleaned.is_empty()).then(|| (key.clone(), cleaned))
        })
        .collect()
}

fn event_files(dir: &Path) -> Vec<(SystemTime, u64, PathBuf)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(EVENT_PREFIX) && name.ends_with(EVENT_SUFFIX))
        })
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            Some((metadata.modified().ok()?, metadata.len(), entry.path()))
        })
        .collect()
}

fn enforce_event_retention(dir: &Path, keep_days: u64, cap: u64) {
    let now = SystemTime::now();
    let max_age = std::time::Duration::from_secs(keep_days.saturating_mul(86_400));
    let mut files = event_files(dir);
    for (modified, _, path) in &files {
        if now
            .duration_since(*modified)
            .map_or(true, |age| age > max_age)
        {
            let _ = fs::remove_file(path);
        }
    }
    files = event_files(dir);
    files.sort_by_key(|(modified, _, _)| *modified);
    let mut total: u64 = files.iter().map(|(_, size, _)| *size).sum();
    for (_, size, path) in files {
        if total <= cap {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

fn recent_events(dir: &Path, period: std::time::Duration) -> Vec<DiagnosticEvent> {
    let now = SystemTime::now();
    let mut files = event_files(dir);
    files.sort_by_key(|(modified, _, _)| *modified);
    files
        .into_iter()
        .filter(|(modified, _, _)| now.duration_since(*modified).is_ok_and(|age| age <= period))
        .flat_map(|(_, _, path)| fs::read_to_string(path).ok().into_iter())
        .flat_map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str::<DiagnosticEvent>(line).ok())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn cap_events(events: Vec<DiagnosticEvent>, limit: usize) -> (Vec<DiagnosticEvent>, bool) {
    let original_count = events.len();
    let mut kept = Vec::new();
    let mut total = 0usize;
    for event in events.into_iter().rev() {
        let size = serde_json::to_vec(&event).map_or(0, |bytes| bytes.len().saturating_add(1));
        if total.saturating_add(size) > limit {
            continue;
        }
        total = total.saturating_add(size);
        kept.push(event);
    }
    let truncated = kept.len() < original_count;
    kept.reverse();
    (kept, truncated)
}

fn json_io(error: serde_json::Error) -> std::io::Error {
    std::io::Error::other(error)
}

fn zip_io(error: zip::result::ZipError) -> std::io::Error {
    std::io::Error::other(error)
}

fn write_zip(path: &Path, entries: [(&str, Vec<u8>); 6]) -> std::io::Result<()> {
    let file = fs::File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, content) in entries {
        zip.start_file(name, options).map_err(zip_io)?;
        zip.write_all(&content)?;
    }
    zip.finish().map_err(zip_io)?;
    Ok(())
}

fn utc_date() -> String {
    let seconds = unix_ms().saturating_div(1_000) as i64;
    let (year, month, day, _, _, _) = civil_utc(seconds);
    format!("{year:04}-{month:02}-{day:02}")
}

fn iso8601_utc(ms: u128) -> String {
    let seconds = ms.saturating_div(1_000) as i64;
    let millis = ms % 1_000;
    let (year, month, day, hour, minute, second) = civil_utc(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// UTC calendar conversion without a date/time dependency. Based on the same civil-day method
/// already used by `storage.rs`, extended with a time-of-day for the portable event schema.
fn civil_utc(seconds: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = seconds.div_euclid(86_400);
    let second_of_day = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        second_of_day / 3_600,
        (second_of_day % 3_600) / 60,
        second_of_day % 60,
    )
}

/// Gives the UI only the non-sensitive lifecycle facts needed to offer a voluntary report.
#[tauri::command(async)]
#[specta::specta]
pub fn diagnostics_status(session: State<'_, DiagnosticSession>) -> DiagnosticStatus {
    session.status()
}

/// Writes a report to the app-local exports folder. The frontend may offer that file for sharing,
/// but it never sends diagnostics itself.
#[tauri::command(async)]
#[specta::specta]
pub fn diagnostics_export(session: State<'_, DiagnosticSession>) -> Result<ReportExport, AppError> {
    session
        .export_report(&data_dir().join("reports"))
        .map_err(|error| AppError::new(ErrorCode::Internal).with_details(error.to_string()))
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

    #[test]
    fn jsonl_events_keep_only_allowlisted_non_sensitive_context() {
        let dir = tmp("event-privacy");
        let session = DiagnosticSession::start(&dir);
        let context = BTreeMap::from([
            ("attempt".to_owned(), "2".to_owned()),
            (
                "file_path".to_owned(),
                "C:\\Users\\Ava\\Music\\private.mp3".to_owned(),
            ),
            (
                "authorization".to_owned(),
                "Bearer super-secret-token".to_owned(),
            ),
        ]);
        session.record(
            "ERROR",
            "IMPORT_OPEN_FAILED",
            "library",
            "open_file",
            Some("FileUnavailable"),
            &context,
            true,
        );
        let file = event_files(&dir.join(DIAGNOSTICS_DIR))
            .pop()
            .expect("event file")
            .2;
        let text = fs::read_to_string(file).expect("event text");
        let event: serde_json::Value = serde_json::from_str(text.trim()).expect("json event");
        assert_eq!(event["technical_context"]["attempt"], "2");
        assert!(event["technical_context"].get("file_path").is_none());
        assert!(!text.contains("private.mp3"));
        assert!(!text.contains("super-secret-token"));
        assert_eq!(
            event["timestamp_utc"].as_str().map(|s| s.ends_with('Z')),
            Some(true)
        );
        session.finish();
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn event_cap_removes_oldest_files_first() {
        let dir = tmp("event-cap");
        for (i, size) in [400usize, 400, 400].iter().enumerate() {
            fs::write(
                dir.join(format!("{EVENT_PREFIX}2026-10-1{i}.{EVENT_SUFFIX}")),
                vec![b'x'; *size],
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        enforce_event_retention(&dir, u64::MAX / 86_400, 900);
        let names: Vec<_> = event_files(&dir)
            .into_iter()
            .filter_map(|(_, _, path)| path.file_name().map(|name| name.to_owned()))
            .collect();
        assert_eq!(names.len(), 2);
        assert!(!names.iter().any(|name| name == "events-2026-10-10.jsonl"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn exported_report_has_required_files_and_no_unapproved_data() {
        use std::io::{Cursor, Read};

        let dir = tmp("report");
        let session = DiagnosticSession::start(&dir);
        let context = BTreeMap::from([
            ("attempt".to_owned(), "1".to_owned()),
            (
                "file_path".to_owned(),
                "C:\\Users\\Ava\\private.flac".to_owned(),
            ),
            ("authorization".to_owned(), "Bearer test-token".to_owned()),
        ]);
        session.record(
            "ERROR",
            "IMPORT_OPEN_FAILED",
            "library",
            "open_file",
            Some("FileUnavailable"),
            &context,
            true,
        );
        fs::OpenOptions::new()
            .append(true)
            .open(dir.join(DIAGNOSTICS_DIR).join(format!(
                "{EVENT_PREFIX}{}.{}",
                utc_date(),
                EVENT_SUFFIX
            )))
            .unwrap()
            .write_all(b"{broken json}\n")
            .unwrap();
        // Restore one valid event after the intentionally corrupt line.
        session.record(
            "INFO",
            "APP_STARTED",
            "lifecycle",
            "startup",
            None,
            &BTreeMap::new(),
            true,
        );
        let report = session.export_report(&dir.join("exports")).unwrap();
        assert!(Uuid::parse_str(&report.report_id).is_ok());
        let bytes = fs::read(&report.path).unwrap();
        assert!(bytes.len() <= REPORT_MAX_BYTES as usize);
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        for name in [
            "summary.txt",
            "manifest.json",
            "environment.json",
            "events.jsonl",
            "errors.json",
            "README.txt",
        ] {
            assert!(zip.by_name(name).is_ok(), "missing {name}");
        }
        let mut events = String::new();
        zip.by_name("events.jsonl")
            .unwrap()
            .read_to_string(&mut events)
            .unwrap();
        assert!(events.contains("IMPORT_OPEN_FAILED"));
        assert!(!events.contains("private.flac"));
        assert!(!events.contains("test-token"));
        session.finish();
        let _ = fs::remove_dir_all(dir);
    }
}
