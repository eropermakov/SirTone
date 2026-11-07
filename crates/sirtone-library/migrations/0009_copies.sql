-- Economy copies on the phone (D-146, stage 8). A track can have its original (`rel_path`), an
-- Opus copy next to it (`copy_rel_path`), or only the copy (`is_copy` = 1: `rel_path` itself is
-- the copy — the original was removed to free space, or the track came from the phone that way).
-- `copy_level`: 'economy' (128 kbit/s) | 'max' (64 kbit/s). The PC never makes copies; an original
-- found later replaces a copy-only track (Q-34).
ALTER TABLE track ADD COLUMN copy_rel_path TEXT;
ALTER TABLE track ADD COLUMN copy_bytes INTEGER;
ALTER TABLE track ADD COLUMN copy_level TEXT CHECK (copy_level IN ('economy','max'));
ALTER TABLE track ADD COLUMN is_copy INTEGER NOT NULL DEFAULT 0;
