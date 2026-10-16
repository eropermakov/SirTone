//! Opening `library.db` (docs/03 §7): pragmas by medium, forward-only migrations, `meta` keys.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use rusqlite_migration::{M, Migrations};

use crate::error::LibraryError;

/// Where the database lives decides the journal mode (docs/03 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Medium {
    /// Fixed disk: WAL + `synchronous=NORMAL`.
    Fixed,
    /// Removable drive (portable mode on a flash drive): rollback journal + `synchronous=FULL`,
    /// no `-wal/-shm` files that could be lost when the stick is pulled.
    Removable,
}

const MIGRATIONS: &[M<'static>] = &[M::up(include_str!("../migrations/0001_init.sql"))];

/// Current schema version (`PRAGMA user_version` and `meta.schema_version`).
pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn pragmas(conn: &Connection, medium: Medium) -> rusqlite::Result<()> {
    let journal = match medium {
        Medium::Fixed => "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;",
        Medium::Removable => "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;",
    };
    conn.execute_batch(journal)?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY;
         PRAGMA mmap_size=268435456; PRAGMA cache_size=-65536; PRAGMA busy_timeout=5000;",
    )
}

/// Opens (or creates) the database, migrates it forward and fills `meta` on first creation.
pub fn open(path: &Path, medium: Medium) -> Result<Connection, LibraryError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut conn = Connection::open(path)?;
    pragmas(&conn, medium)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// In-memory database with the full schema (tests, benchmarks).
pub fn open_in_memory() -> Result<Connection, LibraryError> {
    let mut conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> Result<(), LibraryError> {
    Migrations::from_slice(MIGRATIONS)
        .to_latest(conn)
        .map_err(|e| LibraryError::Migration(e.to_string()))?;
    let tx = conn.transaction()?;
    let existing: Option<String> = tx
        .query_row(
            "SELECT value FROM meta WHERE key = 'library_uid'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_none() {
        tx.execute(
            "INSERT INTO meta(key, value) VALUES ('library_uid', ?1), ('created_at', ?2)",
            params![uuid::Uuid::now_v7().to_string(), now_ms().to_string()],
        )?;
    }
    tx.execute(
        "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SCHEMA_VERSION.to_string()],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn meta(conn: &Connection, key: &str) -> Result<Option<String>, LibraryError> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid_and_idempotent() {
        Migrations::from_slice(MIGRATIONS).validate().unwrap();
        let dir = std::env::temp_dir().join(format!("sirtone-lib-{}", std::process::id()));
        let path = dir.join("library.db");
        let _ = std::fs::remove_dir_all(&dir);
        let uid = {
            let c = open(&path, Medium::Fixed).unwrap();
            let mode: String = c
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
            meta(&c, "library_uid").unwrap().unwrap()
        };
        // Reopen: same library, nothing re-created.
        let c = open(&path, Medium::Fixed).unwrap();
        assert_eq!(meta(&c, "library_uid").unwrap().unwrap(), uid);
        assert_eq!(meta(&c, "schema_version").unwrap().as_deref(), Some("1"));
        let v: i64 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
        drop(c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removable_medium_uses_rollback_journal() {
        let dir = std::env::temp_dir().join(format!("sirtone-lib-rm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let c = open(&dir.join("library.db"), Medium::Removable).unwrap();
        let mode: String = c
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "delete");
        drop(c);
        assert!(!dir.join("library.db-wal").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn schema_has_fts_and_keyset_indexes() {
        let c = open_in_memory().unwrap();
        for name in [
            "search_fts",
            "search_tri",
            "ix_track_title",
            "ix_track_added",
            "fs_journal",
        ] {
            let n: i64 = c
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name = ?1",
                    [name],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "{name}");
        }
    }
}
