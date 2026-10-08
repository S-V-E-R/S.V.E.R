-- Mature label, part 3 (docs/CHANNEL_ADDITIONS.md): games with a mature age rating (ESRB M or AO,
-- PEGI 18, from Wikidata) switch the label on in Studio, and "Should be labeled mature" is a report
-- reason.
ALTER TABLE game_catalog ADD COLUMN mature boolean NOT NULL DEFAULT false;
ALTER TABLE reports DROP CONSTRAINT reports_reason_check;
ALTER TABLE reports ADD CONSTRAINT reports_reason_check CHECK (reason IN ('spam', 'harassment', 'hate', 'sexual', 'violence', 'impersonation', 'private_information', 'copyright', 'mature_label', 'other'));
