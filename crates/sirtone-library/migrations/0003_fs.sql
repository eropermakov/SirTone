-- 0003_fs: file watching and reconcile (docs/05 §7, T1.19–T1.20). Forward-only; frozen once shipped.
-- missing_since: when the file disappeared while the root was online (purged after 24 h).
ALTER TABLE track ADD COLUMN missing_since INTEGER;
-- Files inside the root that are not tracks (not audio, broken, exact copies): remembered by
-- (rel_path, size, mtime) so the watcher does not probe and hash them on every pass.
CREATE TABLE fs_ignore (
  rel_path   TEXT PRIMARY KEY,
  size_bytes INTEGER NOT NULL,
  mtime_ms   INTEGER NOT NULL,
  reason     TEXT NOT NULL,
  ts         INTEGER NOT NULL
) WITHOUT ROWID;
