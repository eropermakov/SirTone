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

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::EnvFilter;

const LOG_PREFIX: &str = "sirtone";
const LOG_SUFFIX: &str = "log";
const KEEP_FILES: usize = 7;
const MAX_TOTAL_BYTES: u64 = 20 * 1024 * 1024;

/// Application data directory: `%LOCALAPPDATA%\SirTone` (installed mode, docs/03 §8.2).
/// Portable mode (`sirtone.portable` next to the exe) is wired in T6.4.
pub fn data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("SirTone")
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
    let filter = EnvFilter::try_from_env("SIRTONE_LOG").unwrap_or_else(|_| EnvFilter::new(default));
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
}
