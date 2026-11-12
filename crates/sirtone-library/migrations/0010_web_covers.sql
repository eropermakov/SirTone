-- D-172: a photo for an artist and a cover for a single track found on the web.
ALTER TABLE artist ADD COLUMN photo_cover_id INTEGER REFERENCES cover(id);
ALTER TABLE track ADD COLUMN cover_id INTEGER REFERENCES cover(id);
