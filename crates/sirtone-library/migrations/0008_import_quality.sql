-- The policy belongs to an import batch, not to global settings: a resumed batch must preserve
-- the quality selected when it was created.
ALTER TABLE import_batch ADD COLUMN quality TEXT NOT NULL DEFAULT 'original'
  CHECK (quality IN ('original', 'high', 'standard', 'compact'));
