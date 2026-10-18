-- 0005_artists: artist merge suggestions the user turned down (T3.12, docs/05 §5, FR-ART-003).
-- Forward-only; frozen once shipped (D-66).

CREATE TABLE artist_pair_dismissed (
  a INTEGER NOT NULL REFERENCES artist(id) ON DELETE CASCADE,
  b INTEGER NOT NULL REFERENCES artist(id) ON DELETE CASCADE,
  PRIMARY KEY (a, b),
  CHECK (a < b)
) WITHOUT ROWID;
