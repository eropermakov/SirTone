-- 0007_loudness: loudness from ReplayGain 2.0 / R128 tags (T4.11, FR-PLAY-008, docs/04 §5).
-- Forward-only; frozen once shipped (D-66). LUFS already converted from the tag's reference
-- (ReplayGain −18, R128 −23); peaks are sample peaks, dBFS. The own measurement (lufs_i,
-- true_peak_db from B0) wins when both exist.
ALTER TABLE track ADD COLUMN tag_lufs REAL;
ALTER TABLE track ADD COLUMN tag_peak_db REAL;
ALTER TABLE track ADD COLUMN tag_lufs_album REAL;
ALTER TABLE track ADD COLUMN tag_peak_album_db REAL;
