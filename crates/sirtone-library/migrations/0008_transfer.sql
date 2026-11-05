-- Transfer PC ↔ phone (D-140, stage 8): keys of journal events and marks already applied from a
-- package, so a package applied twice — or a journal that already travelled back — never
-- doubles anything. Keys are `<library_uid>:<kind>:<id>` of the library that wrote them.
CREATE TABLE transfer_seen (
  key TEXT PRIMARY KEY,
  applied_at INTEGER NOT NULL
) WITHOUT ROWID;
