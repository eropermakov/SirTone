-- 0004_dup: duplicates by sound (T3.5, docs/05 §6). Forward-only; frozen once shipped (D-66).
-- Exact copies never enter the library (import_item.state = 'duplicate'); here — pairs found by
-- fingerprints after the background analysis: «тот же звук» and «похожий (другая версия)».

-- Every 8th raw fingerprint item (≤ 128, u32 LE): the coarse check compares a new track with
-- the candidates' sketches without decompressing their fingerprints.
ALTER TABLE track ADD COLUMN fp_sketch BLOB;

CREATE TABLE duplicate_pair (
  a INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,  -- the earlier track (smaller id)
  b INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,  -- the later one
  class TEXT NOT NULL CHECK (class IN ('same_sound','similar')),
  similarity REAL NOT NULL,                                   -- 1 − bit error rate; 0 for «same name, other sound»
  found_at INTEGER NOT NULL,
  decision TEXT CHECK (decision IN ('keep_both')),            -- «оставить оба» — the pair does not come back
  PRIMARY KEY (a, b),
  CHECK (a < b)
) WITHOUT ROWID;
CREATE INDEX ix_dup_b ON duplicate_pair(b);
