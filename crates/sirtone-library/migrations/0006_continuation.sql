-- 0006_continuation: playlist continuation (T3.14, FR-PLS-003/005, docs/04 §11, D-86).
-- Forward-only; frozen once shipped (D-66).

-- «−» on a recommended track: never offered again for this playlist.
CREATE TABLE playlist_exclude (
  playlist_id INTEGER NOT NULL REFERENCES playlist(id) ON DELETE CASCADE,
  track_id    INTEGER NOT NULL REFERENCES track(id) ON DELETE CASCADE,
  created_at  INTEGER NOT NULL,
  PRIMARY KEY (playlist_id, track_id)
) WITHOUT ROWID;

-- ∞ is on by default for manual playlists (FR-PLS-005); no screen ever set the old 0.
UPDATE playlist SET hybrid = 1 WHERE kind = 'manual';
