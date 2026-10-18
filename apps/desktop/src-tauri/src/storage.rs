//! «Хранилище» (T2.14, FR-DATA-004, `05` §12): what the app keeps besides the music — the
//! library database, the cover cache, the logs — and a cleanup of old logs. Music files are
//! counted by the library status; nothing here touches them.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sirtone_core::error::{AppError, ErrorCode};
use sirtone_library::import::SERVICE_DIR;
use tauri::State;

use crate::diagnostics::data_dir;
use crate::library::Library;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    /// `library.db` with its WAL.
    pub database_bytes: f64,
    pub covers_bytes: f64,
    pub covers_files: u32,
    pub logs_bytes: f64,
    /// Log files older than today (what «Удалить старые журналы» removes).
    pub old_logs: u32,
}

/// (bytes, files) under `dir`, recursively.
fn usage(dir: &Path) -> (u64, u32) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut total = (0u64, 0u32);
    for e in rd.flatten() {
        let Ok(m) = e.metadata() else { continue };
        if m.is_dir() {
            let (b, n) = usage(&e.path());
            total.0 += b;
            total.1 += n;
        } else {
            total.0 += m.len();
            total.1 += 1;
        }
    }
    total
}

/// Rolled log files (`sirtone.YYYY-MM-DD.log`) other than today's.
fn old_logs() -> Vec<std::path::PathBuf> {
    let today = format!("sirtone.{}.log", chrono_today());
    std::fs::read_dir(data_dir().join("logs"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                        n.starts_with("sirtone.") && n.ends_with(".log") && n != today
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Today's date as the log appender names files (UTC, like tracing-appender).
fn chrono_today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    date_of(secs)
}

/// `YYYY-MM-DD` (UTC) of a Unix time.
fn date_of(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[tauri::command(async)]
#[specta::specta]
pub fn storage_info(lib: State<'_, Library>) -> StorageInfo {
    let root = lib.root();
    let service = root.as_ref().map(|r| r.join(SERVICE_DIR));
    let db = service.as_ref().map_or(0, |s| {
        ["library.db", "library.db-wal", "library.db-shm"]
            .iter()
            .filter_map(|n| std::fs::metadata(s.join(n)).ok())
            .map(|m| m.len())
            .sum()
    });
    let (covers_bytes, covers_files) = service
        .as_ref()
        .map_or((0, 0), |s| usage(&s.join("covers")));
    let (logs_bytes, _) = usage(&data_dir().join("logs"));
    StorageInfo {
        database_bytes: db as f64,
        covers_bytes: covers_bytes as f64,
        covers_files,
        logs_bytes: logs_bytes as f64,
        old_logs: u32::try_from(old_logs().len()).unwrap_or(u32::MAX),
    }
}

/// «Удалить старые журналы»: diagnostic logs of earlier days (the app's own files, not music).
#[tauri::command(async)]
#[specta::specta]
pub fn storage_clear_logs() -> Result<u32, AppError> {
    let mut n = 0;
    for p in old_logs() {
        std::fs::remove_file(&p)
            .map_err(|e| AppError::new(ErrorCode::Internal).with_details(e.to_string()))?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_match_the_log_file_names() {
        assert_eq!(super::date_of(0), "1970-01-01");
        assert_eq!(super::date_of(1_700_000_000), "2023-11-14");
        assert_eq!(super::date_of(951_782_400), "2000-02-29");
    }
}
