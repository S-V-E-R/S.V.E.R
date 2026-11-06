-- Game detection (docs/LIVE_STREAMS.md "Game detection"): unknown checks (menus, lobbies) seen while a
-- suggestion builds, so it needs 3 reads of the same game within 4 checks in a row.
ALTER TABLE broadcasts ADD COLUMN detect_gaps integer NOT NULL DEFAULT 0;
